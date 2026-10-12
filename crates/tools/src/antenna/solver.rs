use num_complex::Complex64;

use super::structure::{Pulse, Segment, Structure, Vec3};

pub(super) const SPEED_OF_LIGHT_M_S: f64 = 299_792_458.0;
pub(super) const FREE_SPACE_OHMS: f64 = 376.730_313_412;
const MU_0: f64 = 1.256_637_062_12e-6;

const GAUSS_NODES: [f64; 4] = [
    -0.861_136_311_594_052_6,
    -0.339_981_043_584_856_3,
    0.339_981_043_584_856_3,
    0.861_136_311_594_052_6,
];
const GAUSS_WEIGHTS: [f64; 4] = [
    0.347_854_845_137_453_9,
    0.652_145_154_862_546_1,
    0.652_145_154_862_546_1,
    0.347_854_845_137_453_9,
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Medium {
    pub frequency_hz: f64,
    pub radius_m: f64,
    pub conductivity_s_m: Option<f64>,
    pub image: bool,
}

impl Medium {
    pub fn wavenumber(&self) -> f64 {
        std::f64::consts::TAU * self.frequency_hz / SPEED_OF_LIGHT_M_S
    }

    pub fn loss_ohms(&self, length_m: f64) -> f64 {
        self.conductivity_s_m.map_or(0.0, |sigma| {
            let surface = (std::f64::consts::PI * self.frequency_hz * MU_0 / sigma).sqrt();
            surface * length_m / (std::f64::consts::TAU * self.radius_m)
        })
    }
}

#[derive(Clone, Debug)]
pub(super) struct Solution {
    pub currents: Vec<Complex64>,
    pub feed_current: Complex64,
}

impl Solution {
    pub fn impedance(&self) -> Complex64 {
        Complex64::new(1.0, 0.0) / self.feed_current
    }

    pub fn input_power_w(&self) -> f64 {
        0.5 * self.feed_current.conj().re
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Singular;

pub(super) fn solve(structure: &Structure, medium: &Medium) -> Result<Solution, Singular> {
    let size = structure.pulses.len();
    let mut matrix = fill(structure, medium);
    let mut rhs = vec![Complex64::new(0.0, 0.0); size];
    for &feed in &structure.feeds {
        rhs[feed] = Complex64::new(1.0, 0.0);
    }
    lu_solve(&mut matrix, &mut rhs, size)?;
    let feed_current: Complex64 = structure.feeds.iter().map(|&feed| rhs[feed]).sum();
    if !feed_current.is_finite() || feed_current.norm() == 0.0 {
        return Err(Singular);
    }
    Ok(Solution {
        currents: rhs,
        feed_current,
    })
}

struct Kernel<'a> {
    structure: &'a Structure,
    k: f64,
    radius: f64,
    mirror: bool,
}

impl Kernel<'_> {
    fn segment(&self, index: usize) -> Segment {
        let segment = self.structure.segments[index];
        if self.mirror {
            segment.reflected()
        } else {
            segment
        }
    }

    fn charge_table(&self) -> Vec<Complex64> {
        let count = self.structure.segments.len();
        let mut table = Vec::with_capacity(count * count);
        for observer in &self.structure.segments {
            let at = observer.middle();
            for source in 0..count {
                let segment = self.segment(source);
                table.push(psi(at, segment.start, segment.end, self.radius, self.k));
            }
        }
        table
    }

    fn vector_term(&self, at: Vec3, along: Vec3, source: &Pulse) -> Complex64 {
        source
            .halves
            .iter()
            .map(|&half| {
                let (start, end) = self.structure.half_ends(half);
                let (start, end) = if self.mirror {
                    (start.reflected(), end.reflected())
                } else {
                    (start, end)
                };
                let span = end - start;
                let length = span.norm();
                if length == 0.0 {
                    return Complex64::new(0.0, 0.0);
                }
                psi(at, start, end, self.radius, self.k) * (span.dot(along) / length)
            })
            .sum()
    }
}

fn fill(structure: &Structure, medium: &Medium) -> Vec<Complex64> {
    let k = medium.wavenumber();
    let size = structure.pulses.len();
    let mut matrix = vec![Complex64::new(0.0, 0.0); size * size];
    let mut kernels = vec![Kernel {
        structure,
        k,
        radius: medium.radius_m,
        mirror: false,
    }];
    if medium.image {
        kernels.push(Kernel {
            structure,
            k,
            radius: medium.radius_m,
            mirror: true,
        });
    }
    let scale = Complex64::new(0.0, FREE_SPACE_OHMS / (4.0 * std::f64::consts::PI));
    let segments = structure.segments.len();
    for (index, kernel) in kernels.iter().enumerate() {
        let sign = if index == 0 { 1.0 } else { -1.0 };
        let charges = kernel.charge_table();
        for (row, observer) in structure.pulses.iter().enumerate() {
            let start = observer.from_segment;
            let end = observer.to_segment;
            let along = structure.segments[end].middle() - structure.segments[start].middle();
            for (column, source) in structure.pulses.iter().enumerate() {
                let vector = kernel.vector_term(observer.node, along, source);
                let potential = |cell: usize| {
                    (charges[end * segments + cell] - charges[start * segments + cell])
                        / structure.segments[cell].length()
                };
                let scalar = potential(source.to_segment) - potential(source.from_segment);
                matrix[row * size + column] += scale * (vector * k - scalar / k) * sign;
            }
        }
    }
    for (row, pulse) in structure.pulses.iter().enumerate() {
        matrix[row * size + row] += medium.loss_ohms(structure.length_of(pulse));
    }
    matrix
}

pub(super) fn psi(at: Vec3, start: Vec3, end: Vec3, radius: f64, k: f64) -> Complex64 {
    let span = end - start;
    let length = span.norm();
    if length == 0.0 {
        return Complex64::new(0.0, 0.0);
    }
    let axis = span * (1.0 / length);
    let offset = at - start;
    let t = offset.dot(axis);
    let rho = (offset.dot(offset) - t * t).max(0.0) + radius * radius;
    let rho = rho.sqrt();
    let singular = ((length - t) / rho).asinh() + (t / rho).asinh();
    let half = length / 2.0;
    let smooth: Complex64 = GAUSS_NODES
        .iter()
        .zip(GAUSS_WEIGHTS)
        .map(|(&node, weight)| {
            let along = half * (node + 1.0) - t;
            let distance = (rho * rho + along * along).sqrt();
            let phase = Complex64::new(0.0, -k * distance).exp() - 1.0;
            phase / distance * weight
        })
        .sum::<Complex64>()
        * half;
    smooth + singular
}

fn lu_solve(matrix: &mut [Complex64], rhs: &mut [Complex64], size: usize) -> Result<(), Singular> {
    for pivot in 0..size {
        let best = (pivot..size)
            .max_by(|&a, &b| {
                matrix[a * size + pivot]
                    .norm()
                    .total_cmp(&matrix[b * size + pivot].norm())
            })
            .ok_or(Singular)?;
        let lead = matrix[best * size + pivot];
        if lead.norm() < 1e-300 || !lead.is_finite() {
            return Err(Singular);
        }
        if best != pivot {
            for column in 0..size {
                matrix.swap(best * size + column, pivot * size + column);
            }
            rhs.swap(best, pivot);
        }
        for row in pivot + 1..size {
            let factor = matrix[row * size + pivot] / lead;
            if factor.norm() == 0.0 {
                continue;
            }
            for column in pivot..size {
                let above = matrix[pivot * size + column];
                matrix[row * size + column] -= factor * above;
            }
            let carried = rhs[pivot];
            rhs[row] -= factor * carried;
        }
    }
    for row in (0..size).rev() {
        let mut value = rhs[row];
        for column in row + 1..size {
            value -= matrix[row * size + column] * rhs[column];
        }
        rhs[row] = value / matrix[row * size + row];
    }
    Ok(())
}
