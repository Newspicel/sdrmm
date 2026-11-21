use num_complex::Complex;
use sdrmm_dsp::{CONFIDENT, Soft};

use super::en300744::{CONTINUAL_PILOTS_2K, CONTINUAL_PILOTS_8K, TPS_CARRIERS_2K, TPS_CARRIERS_8K};

const SHIFTS: [usize; 6] = [0, 63, 105, 42, 21, 84];

pub struct Mapping {
    pub fft: usize,
    pub carriers: usize,
    pub data: [Vec<usize>; 4],
    pub continual: Vec<usize>,
    pub tps: Vec<usize>,
    pub reference: Vec<f32>,
    pub permutation: Vec<usize>,
}

impl Mapping {
    pub fn new(fft: usize) -> Self {
        let carriers = 1704 * (fft / 2048) + 1;
        let (continual, tps) = if fft == 2048 {
            (CONTINUAL_PILOTS_2K.to_vec(), TPS_CARRIERS_2K.to_vec())
        } else {
            (CONTINUAL_PILOTS_8K.to_vec(), TPS_CARRIERS_8K.to_vec())
        };
        let mut register = 0x7ffu16;
        let reference = (0..carriers)
            .map(|_| {
                let value = if register & 1 == 0 { 1.0 } else { -1.0 };
                register = (register >> 1) | (((register ^ (register >> 2)) & 1) << 10);
                value
            })
            .collect();
        let pilots: [Vec<usize>; 4] = std::array::from_fn(|phase| {
            (0..carriers)
                .filter(|k| k % 12 == 3 * phase || continual.binary_search(k).is_ok())
                .collect::<Vec<_>>()
        });
        let data = std::array::from_fn(|phase| {
            (0..carriers)
                .filter(|k| {
                    pilots[phase].binary_search(k).is_err() && tps.binary_search(k).is_err()
                })
                .collect()
        });
        Self {
            fft,
            carriers,
            data,
            continual,
            tps,
            reference,
            permutation: permutation(fft),
        }
    }

    #[inline]
    pub fn bin(&self, carrier: usize, offset: isize) -> usize {
        debug_assert!(self.fft.is_power_of_two());
        (carrier as isize - (self.carriers / 2) as isize + offset) as usize & (self.fft - 1)
    }
}

pub fn permutation(fft: usize) -> Vec<usize> {
    let order: &[usize] = if fft == 2048 {
        &[4, 3, 9, 6, 2, 8, 1, 5, 7, 0]
    } else {
        &[7, 1, 4, 2, 9, 6, 8, 10, 0, 3, 11, 5]
    };
    let mut state = 0usize;
    let mut result = Vec::with_capacity(fft * 189 / 256);
    for i in 0..fft {
        state = match i {
            0 | 1 => 0,
            2 => 1,
            _ => {
                let feedback = if fft == 2048 {
                    state ^ (state >> 3)
                } else {
                    state ^ (state >> 1) ^ (state >> 4) ^ (state >> 6)
                };
                (state >> 1) | ((feedback & 1) << (order.len() - 1))
            }
        };
        let mapped = order
            .iter()
            .enumerate()
            .fold((i & 1) * fft / 2, |value, (source, &dest)| {
                value | (((state >> source) & 1) << dest)
            });
        if mapped < fft * 189 / 256 {
            result.push(mapped);
        }
    }
    result
}

pub fn point(word: usize, bits: usize, alpha: usize) -> Complex<f32> {
    let axis = |parity: usize| {
        let sign = if word & (1 << (bits - 1 - parity)) == 0 {
            1.0
        } else {
            -1.0
        };
        let gray = (2 + parity..bits)
            .step_by(2)
            .fold(0usize, |g, bit| (g << 1) | ((word >> (bits - 1 - bit)) & 1));
        let mut binary = gray;
        let mut part = gray >> 1;
        while part != 0 {
            binary ^= part;
            part >>= 1;
        }
        sign * (alpha + 2 * ((1 << (bits / 2 - 1)) - 1 - binary)) as f32
    };
    let levels = 1 << (bits / 2 - 1);
    let power = 2.0
        * (0..levels)
            .map(|i| ((alpha + 2 * i) as f32).powi(2))
            .sum::<f32>()
        / levels as f32;
    Complex::new(axis(0), axis(1)) / power.sqrt()
}

#[inline]
pub fn soften(
    value: Complex<f32>,
    table: &[Complex<f32>],
    bits: usize,
    scale: f32,
) -> ([Soft; 6], f32) {
    let limit = f32::from(CONFIDENT);
    let clamp_soft = |diff: f32| (diff * scale).clamp(-limit, limit) as Soft;
    match bits {
        2 => {
            let a0 = table[0].re;
            let a1 = table[2].re;
            let dr0 = (value.re - a0) * (value.re - a0);
            let dr1 = (value.re - a1) * (value.re - a1);
            let di0 = (value.im - a0) * (value.im - a0);
            let di1 = (value.im - a1) * (value.im - a1);
            let nearest = dr0.min(dr1) + di0.min(di1);
            (
                [clamp_soft(dr0 - dr1), clamp_soft(di0 - di1), 0, 0, 0, 0],
                nearest,
            )
        }
        4 => {
            let levels = [table[0].re, table[2].re, table[8].re, table[10].re];
            let dr = levels.map(|a| (value.re - a) * (value.re - a));
            let di = levels.map(|a| (value.im - a) * (value.im - a));
            let r01 = dr[0].min(dr[1]);
            let r23 = dr[2].min(dr[3]);
            let i01 = di[0].min(di[1]);
            let i23 = di[2].min(di[3]);
            let nearest = r01.min(r23) + i01.min(i23);
            (
                [
                    clamp_soft(r01 - r23),
                    clamp_soft(i01 - i23),
                    clamp_soft(dr[0].min(dr[2]) - dr[1].min(dr[3])),
                    clamp_soft(di[0].min(di[2]) - di[1].min(di[3])),
                    0,
                    0,
                ],
                nearest,
            )
        }
        _ => {
            let levels = [
                table[0].re,
                table[2].re,
                table[8].re,
                table[10].re,
                table[32].re,
                table[34].re,
                table[40].re,
                table[42].re,
            ];
            let dr = levels.map(|a| (value.re - a) * (value.re - a));
            let di = levels.map(|a| (value.im - a) * (value.im - a));
            let r01 = dr[0].min(dr[1]);
            let r23 = dr[2].min(dr[3]);
            let r45 = dr[4].min(dr[5]);
            let r67 = dr[6].min(dr[7]);
            let r03 = r01.min(r23);
            let r47 = r45.min(r67);

            let i01 = di[0].min(di[1]);
            let i23 = di[2].min(di[3]);
            let i45 = di[4].min(di[5]);
            let i67 = di[6].min(di[7]);
            let i03 = i01.min(i23);
            let i47 = i45.min(i67);

            let nearest = r03.min(r47) + i03.min(i47);
            (
                [
                    clamp_soft(r03 - r47),
                    clamp_soft(i03 - i47),
                    clamp_soft(r01.min(r45) - r23.min(r67)),
                    clamp_soft(i01.min(i45) - i23.min(i67)),
                    clamp_soft(
                        dr[0].min(dr[2]).min(dr[4].min(dr[6]))
                            - dr[1].min(dr[3]).min(dr[5].min(dr[7])),
                    ),
                    clamp_soft(
                        di[0].min(di[2]).min(di[4].min(di[6]))
                            - di[1].min(di[3]).min(di[5].min(di[7])),
                    ),
                ],
                nearest,
            )
        }
    }
}

pub fn deinterleave(
    input: &[[Soft; 6]],
    bits: usize,
    hierarchy: bool,
    low: bool,
    output: &mut Vec<Soft>,
) {
    let lanes: &[usize] = match (bits, hierarchy, low) {
        (_, true, false) => &[0, 1],
        (4, true, true) => &[2, 3],
        (6, true, true) => &[2, 4, 3, 5],
        (2, _, _) => &[0, 1],
        (4, _, _) => &[0, 2, 1, 3],
        _ => &[0, 2, 4, 1, 3, 5],
    };
    let blocks = input.as_chunks::<126>().0;
    let total = blocks.len() * 126 * lanes.len();
    output.clear();
    output.reserve(total);
    let dst = output.spare_capacity_mut();
    let mut cursor = 0;
    let lane_shifts: [usize; 6] = std::array::from_fn(|k| {
        if k < lanes.len() {
            126 - SHIFTS[lanes[k]]
        } else {
            0
        }
    });
    match *lanes {
        [l0, l1] => {
            let (s0, s1) = (lane_shifts[0], lane_shifts[1]);
            for block in blocks {
                for i in 0..126 {
                    let i0 = if i + s0 >= 126 { i + s0 - 126 } else { i + s0 };
                    let i1 = if i + s1 >= 126 { i + s1 - 126 } else { i + s1 };
                    dst[cursor].write(block[i0][l0]);
                    dst[cursor + 1].write(block[i1][l1]);
                    cursor += 2;
                }
            }
        }
        [l0, l1, l2, l3] => {
            let (s0, s1, s2, s3) = (
                lane_shifts[0],
                lane_shifts[1],
                lane_shifts[2],
                lane_shifts[3],
            );
            for block in blocks {
                for i in 0..126 {
                    let i0 = if i + s0 >= 126 { i + s0 - 126 } else { i + s0 };
                    let i1 = if i + s1 >= 126 { i + s1 - 126 } else { i + s1 };
                    let i2 = if i + s2 >= 126 { i + s2 - 126 } else { i + s2 };
                    let i3 = if i + s3 >= 126 { i + s3 - 126 } else { i + s3 };
                    dst[cursor].write(block[i0][l0]);
                    dst[cursor + 1].write(block[i1][l1]);
                    dst[cursor + 2].write(block[i2][l2]);
                    dst[cursor + 3].write(block[i3][l3]);
                    cursor += 4;
                }
            }
        }
        [l0, l1, l2, l3, l4, l5] => {
            let [s0, s1, s2, s3, s4, s5] = lane_shifts;
            for block in blocks {
                for i in 0..126 {
                    let i0 = if i + s0 >= 126 { i + s0 - 126 } else { i + s0 };
                    let i1 = if i + s1 >= 126 { i + s1 - 126 } else { i + s1 };
                    let i2 = if i + s2 >= 126 { i + s2 - 126 } else { i + s2 };
                    let i3 = if i + s3 >= 126 { i + s3 - 126 } else { i + s3 };
                    let i4 = if i + s4 >= 126 { i + s4 - 126 } else { i + s4 };
                    let i5 = if i + s5 >= 126 { i + s5 - 126 } else { i + s5 };
                    dst[cursor].write(block[i0][l0]);
                    dst[cursor + 1].write(block[i1][l1]);
                    dst[cursor + 2].write(block[i2][l2]);
                    dst[cursor + 3].write(block[i3][l3]);
                    dst[cursor + 4].write(block[i4][l4]);
                    dst[cursor + 5].write(block[i5][l5]);
                    cursor += 6;
                }
            }
        }
        _ => unreachable!(),
    }
    unsafe { output.set_len(cursor) };
}
