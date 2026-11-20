use sdrmm_modem::ppm::PpmDemod;

use super::{PHASE_TABLES, PREAMBLE_PULSES};

pub(super) type Phases = u8;

const _: () = assert!(PHASE_TABLES <= Phases::BITS as usize);

const TILE: usize = 512;
const SCREENED_GAPS: [usize; 4] = [4, 5, 12, 14];

struct Stream {
    weights: Vec<f32>,
    reach: usize,
    values: Vec<f32>,
}

impl Stream {
    fn fill(&mut self, mag: &[f32], from: usize, n: usize) -> Option<()> {
        let len = n + self.reach;
        let samples = mag.get(from..from + len + self.weights.len().saturating_sub(1))?;
        let values = self.values.get_mut(..len)?;
        match *self.weights.as_slice() {
            [w0, w1] => fill_2(values, samples, w0, w1),
            [w0, w1, w2] => fill_3(values, samples, w0, w1, w2),
            _ => {
                values.fill(0.0);
                for (k, &w) in self.weights.iter().enumerate() {
                    for (v, &x) in values.iter_mut().zip(&samples[k..]) {
                        *v = sdrmm_dsp::madd(w, x, *v);
                    }
                }
            }
        }
        Some(())
    }
}

fn fill_2(values: &mut [f32], samples: &[f32], w0: f32, w1: f32) {
    let mut done = 0;
    #[cfg(target_arch = "aarch64")]
    {
        use std::arch::aarch64::{vdupq_n_f32, vfmaq_f32, vld1q_f32, vmulq_f32, vst1q_f32};
        let n = values.len() & !7;
        unsafe {
            let sp = samples.as_ptr();
            let dp = values.as_mut_ptr();
            let vw0 = vdupq_n_f32(w0);
            let vw1 = vdupq_n_f32(w1);
            while done < n {
                let x0 = vld1q_f32(sp.add(done));
                let x1 = vld1q_f32(sp.add(done + 1));
                let x2 = vld1q_f32(sp.add(done + 4));
                let x3 = vld1q_f32(sp.add(done + 5));
                vst1q_f32(dp.add(done), vfmaq_f32(vmulq_f32(x0, vw0), x1, vw1));
                vst1q_f32(dp.add(done + 4), vfmaq_f32(vmulq_f32(x2, vw0), x3, vw1));
                done += 8;
            }
            while done + 4 <= values.len() {
                let x0 = vld1q_f32(sp.add(done));
                let x1 = vld1q_f32(sp.add(done + 1));
                vst1q_f32(dp.add(done), vfmaq_f32(vmulq_f32(x0, vw0), x1, vw1));
                done += 4;
            }
        }
    }
    for ((v, &x0), &x1) in values[done..]
        .iter_mut()
        .zip(&samples[done..])
        .zip(&samples[done + 1..])
    {
        *v = sdrmm_dsp::madd(w1, x1, w0 * x0);
    }
}

fn fill_3(values: &mut [f32], samples: &[f32], w0: f32, w1: f32, w2: f32) {
    let mut done = 0;
    #[cfg(target_arch = "aarch64")]
    {
        use std::arch::aarch64::{vdupq_n_f32, vfmaq_f32, vld1q_f32, vmulq_f32, vst1q_f32};
        let n = values.len() & !7;
        unsafe {
            let sp = samples.as_ptr();
            let dp = values.as_mut_ptr();
            let vw0 = vdupq_n_f32(w0);
            let vw1 = vdupq_n_f32(w1);
            let vw2 = vdupq_n_f32(w2);
            while done < n {
                let a0 = vld1q_f32(sp.add(done));
                let a1 = vld1q_f32(sp.add(done + 1));
                let a2 = vld1q_f32(sp.add(done + 2));
                let b0 = vld1q_f32(sp.add(done + 4));
                let b1 = vld1q_f32(sp.add(done + 5));
                let b2 = vld1q_f32(sp.add(done + 6));
                let r0 = vfmaq_f32(vfmaq_f32(vmulq_f32(a0, vw0), a1, vw1), a2, vw2);
                let r1 = vfmaq_f32(vfmaq_f32(vmulq_f32(b0, vw0), b1, vw1), b2, vw2);
                vst1q_f32(dp.add(done), r0);
                vst1q_f32(dp.add(done + 4), r1);
                done += 8;
            }
            while done + 4 <= values.len() {
                let a0 = vld1q_f32(sp.add(done));
                let a1 = vld1q_f32(sp.add(done + 1));
                let a2 = vld1q_f32(sp.add(done + 2));
                let r = vfmaq_f32(vfmaq_f32(vmulq_f32(a0, vw0), a1, vw1), a2, vw2);
                vst1q_f32(dp.add(done), r);
                done += 4;
            }
        }
    }
    for (((v, &x0), &x1), &x2) in values[done..]
        .iter_mut()
        .zip(&samples[done..])
        .zip(&samples[done + 1..])
        .zip(&samples[done + 2..])
    {
        *v = sdrmm_dsp::madd(w2, x2, sdrmm_dsp::madd(w1, x1, w0 * x0));
    }
}

#[derive(Clone, Copy)]
struct ChipRef {
    stream: usize,
    start: usize,
}

struct PhaseChips {
    pulses: [ChipRef; PREAMBLE_PULSES.len()],
    gaps: [ChipRef; SCREENED_GAPS.len()],
}

impl PhaseChips {
    fn mark(&self, streams: &[Stream], phase: usize, hits: &mut [Phases]) {
        let len = hits.len();
        let (Some(p), Some(g)) = (
            slices(&self.pulses, streams, len),
            slices(&self.gaps, streams, len),
        ) else {
            for hit in &mut *hits {
                *hit |= 1 << phase;
            }
            return;
        };
        let bit = 1u8 << phase;
        let mut i = 0;
        #[cfg(target_arch = "aarch64")]
        {
            use std::arch::aarch64::{
                vaddq_f32, vandq_u32, vcgtq_f32, vcltq_f32, vdupq_n_f32, vld1q_f32, vmaxq_f32,
                vmaxvq_u32, vminq_f32, vmulq_f32, vorrq_u32, vst1q_u32,
            };
            let n16 = len & !15;
            unsafe {
                let scale = vdupq_n_f32(0.125);
                let (p0, p1, p2, p3) = (p[0].as_ptr(), p[1].as_ptr(), p[2].as_ptr(), p[3].as_ptr());
                let (g0, g1, g2, g3) = (g[0].as_ptr(), g[1].as_ptr(), g[2].as_ptr(), g[3].as_ptr());
                let check = |at: usize| {
                    let vp0 = vld1q_f32(p0.add(at));
                    let vp1 = vld1q_f32(p1.add(at));
                    let vp2 = vld1q_f32(p2.add(at));
                    let vp3 = vld1q_f32(p3.add(at));
                    let weakest = vminq_f32(vminq_f32(vp0, vp1), vminq_f32(vp2, vp3));
                    let threshold =
                        vmulq_f32(vaddq_f32(vaddq_f32(vp0, vp1), vaddq_f32(vp2, vp3)), scale);
                    let vg0 = vld1q_f32(g0.add(at));
                    let vg1 = vld1q_f32(g1.add(at));
                    let vg2 = vld1q_f32(g2.add(at));
                    let vg3 = vld1q_f32(g3.add(at));
                    let loudest = vmaxq_f32(vmaxq_f32(vg0, vg1), vmaxq_f32(vg2, vg3));
                    vandq_u32(vcgtq_f32(weakest, threshold), vcltq_f32(loudest, threshold))
                };
                while i < n16 {
                    let m0 = check(i);
                    let m1 = check(i + 4);
                    let m2 = check(i + 8);
                    let m3 = check(i + 12);
                    let any = vorrq_u32(vorrq_u32(m0, m1), vorrq_u32(m2, m3));
                    if vmaxvq_u32(any) != 0 {
                        let mut lanes = [0u32; 16];
                        vst1q_u32(lanes.as_mut_ptr(), m0);
                        vst1q_u32(lanes.as_mut_ptr().add(4), m1);
                        vst1q_u32(lanes.as_mut_ptr().add(8), m2);
                        vst1q_u32(lanes.as_mut_ptr().add(12), m3);
                        for (k, &lane) in lanes.iter().enumerate() {
                            if lane != 0 {
                                hits[i + k] |= bit;
                            }
                        }
                    }
                    i += 16;
                }
            }
        }
        while i < len {
            let weakest = p[0][i].min(p[1][i]).min(p[2][i]).min(p[3][i]);
            let threshold = (p[0][i] + p[1][i] + p[2][i] + p[3][i]) * 0.125;
            if weakest > threshold {
                let loudest_gap = g[0][i].max(g[1][i]).max(g[2][i]).max(g[3][i]);
                if loudest_gap < threshold {
                    hits[i] |= bit;
                }
            }
            i += 1;
        }
    }
}

fn slices<'a, const N: usize>(
    chips: &[ChipRef; N],
    streams: &'a [Stream],
    len: usize,
) -> Option<[&'a [f32]; N]> {
    let mut out: [&'a [f32]; N] = [&[]; N];
    for (tile, chip) in out.iter_mut().zip(chips) {
        *tile = streams
            .get(chip.stream)?
            .values
            .get(chip.start..chip.start + len)?;
    }
    Some(out)
}

pub(super) struct PreambleScreen {
    streams: Vec<Stream>,
    phases: Vec<PhaseChips>,
    hits: Vec<Phases>,
}

impl PreambleScreen {
    pub(super) fn new(receivers: &[PpmDemod]) -> Self {
        let mut streams: Vec<Stream> = Vec::new();
        let phases = receivers
            .iter()
            .map(|receiver| {
                let mut chip = |slot| share(&mut streams, receiver, slot);
                PhaseChips {
                    pulses: PREAMBLE_PULSES.map(&mut chip),
                    gaps: SCREENED_GAPS.map(&mut chip),
                }
            })
            .collect();
        for stream in &mut streams {
            stream.values = vec![0.0; TILE + stream.reach];
        }
        Self {
            streams,
            phases,
            hits: Vec::new(),
        }
    }

    pub(super) fn scan(&mut self, mag: &[f32], positions: usize) {
        self.hits.clear();
        self.hits.resize(positions, 0);
        for (tile, hits) in self.hits.chunks_mut(TILE).enumerate() {
            let from = tile * TILE;
            if self
                .streams
                .iter_mut()
                .any(|stream| stream.fill(mag, from, hits.len()).is_none())
            {
                hits.fill(Phases::MAX);
                continue;
            }
            for (phase, chips) in self.phases.iter().enumerate() {
                chips.mark(&self.streams, phase, hits);
            }
        }
    }

    pub(super) fn next(&self, from: usize) -> Option<(usize, Phases)> {
        let hits = self.hits.get(from..)?;
        let skip = hits.iter().position(|&hit| hit != 0)?;
        Some((from + skip, hits[skip]))
    }
}

fn share(streams: &mut Vec<Stream>, receiver: &PpmDemod, slot: usize) -> ChipRef {
    let (start, weights) = receiver.grid().slot_weights(slot);
    let index = streams
        .iter()
        .position(|stream| stream.weights == weights)
        .unwrap_or_else(|| {
            streams.push(Stream {
                weights: weights.to_vec(),
                reach: 0,
                values: Vec::new(),
            });
            streams.len() - 1
        });
    if let Some(stream) = streams.get_mut(index) {
        stream.reach = stream.reach.max(start);
    }
    ChipRef {
        stream: index,
        start,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        adsb::{INPUT_RATE_HZ, phase_tables, preamble_ok},
        synth::adsb::{me_identification, squitter, transmission_at_phase},
    };
    use sdrmm_modem::ppm::magnitudes;
    use sdrmm_modem::synth::add_noise;

    #[test]
    fn every_position_a_phase_accepts_survives_the_screen() {
        let receivers = phase_tables(INPUT_RATE_HZ);
        let span = receivers.iter().map(|r| r.grid().span()).max().unwrap();
        let mut screen = PreambleScreen::new(&receivers);
        let frames: Vec<Vec<u8>> = (0..40)
            .map(|k| squitter(0xA0_0000 + k, me_identification("SCREEN")))
            .collect();
        for (seed, amp) in [(1, 0.02), (2, 0.15), (3, 0.3)] {
            let phase = f64::from(seed) / 7.0;
            let mut iq = transmission_at_phase(&frames, 40.0, 0.5, INPUT_RATE_HZ, phase);
            add_noise(&mut iq, seed, amp);
            let mut mag = Vec::new();
            magnitudes(&iq, &mut mag);
            let positions = mag.len() + 1 - span;
            screen.scan(&mag, positions);
            let mut accepted = 0;
            let mut screened = 0;
            for at in 0..positions {
                let kept = match screen.next(at) {
                    Some((hit, phases)) if hit == at => phases,
                    _ => 0,
                };
                for (phase, r) in receivers.iter().enumerate() {
                    let exact = preamble_ok(r, &mag[at..at + r.grid().span()]);
                    assert!(
                        !exact || kept >> phase & 1 == 1,
                        "seed {seed}: {at} lost phase {phase}"
                    );
                    accepted += usize::from(exact);
                }
                screened += usize::from(kept != 0);
            }
            assert!(accepted > 0, "seed {seed}: no preamble");
            assert!(
                screened * 50 < positions,
                "seed {seed}: {screened} of {positions} kept"
            );
        }
    }

    #[test]
    fn a_tile_without_samples_is_kept_whole() {
        let receivers = phase_tables(INPUT_RATE_HZ);
        let mut screen = PreambleScreen::new(&receivers);
        screen.scan(&[0.0; 4], 3);
        assert_eq!(screen.next(0), Some((0, Phases::MAX)));
        assert_eq!(screen.next(2), Some((2, Phases::MAX)));
        assert_eq!(screen.next(3), None);
    }
}
