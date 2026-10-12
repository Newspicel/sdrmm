use num_complex::Complex64;

use super::solver::{FREE_SPACE_OHMS, Solution};
use super::structure::{Structure, Vec3};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Reflection {
    None,
    Perfect,
    Lossy { permittivity: Complex64 },
}

impl Reflection {
    pub fn real(relative_permittivity: f64, conductivity_s_m: f64, frequency_hz: f64) -> Self {
        let omega_epsilon = std::f64::consts::TAU * frequency_hz * 8.854_187_812_8e-12;
        Self::Lossy {
            permittivity: Complex64::new(relative_permittivity, -conductivity_s_m / omega_epsilon),
        }
    }

    pub fn grounded(self) -> bool {
        self != Self::None
    }

    fn coefficients(self, elevation: f64) -> (Complex64, Complex64) {
        match self {
            Self::None => (Complex64::new(0.0, 0.0), Complex64::new(0.0, 0.0)),
            Self::Perfect => (Complex64::new(1.0, 0.0), Complex64::new(1.0, 0.0)),
            Self::Lossy { permittivity } => {
                let sine = elevation.sin();
                let cosine = elevation.cos();
                let root = (permittivity - cosine * cosine).sqrt();
                let vertical = (permittivity * sine - root) / (permittivity * sine + root);
                let horizontal = (sine - root) / (sine + root);
                (vertical, -horizontal)
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Moment {
    center: Vec3,
    current: [Complex64; 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Direction {
    pub azimuth: f64,
    pub elevation: f64,
}

impl Direction {
    pub fn degrees(azimuth_deg: f64, elevation_deg: f64) -> Self {
        Self {
            azimuth: azimuth_deg.to_radians(),
            elevation: elevation_deg.to_radians(),
        }
    }

    fn unit(self) -> Vec3 {
        let flat = self.elevation.cos();
        Vec3::new(
            flat * self.azimuth.sin(),
            self.elevation.sin(),
            flat * self.azimuth.cos(),
        )
    }

    fn up(self) -> Vec3 {
        let rise = self.elevation.sin();
        Vec3::new(
            -rise * self.azimuth.sin(),
            self.elevation.cos(),
            -rise * self.azimuth.cos(),
        )
    }

    fn across(self) -> Vec3 {
        Vec3::new(self.azimuth.cos(), 0.0, -self.azimuth.sin())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Sample {
    pub gain: f64,
    pub vertical_share: f64,
}

pub(super) struct FarField {
    direct: Vec<Moment>,
    image: Vec<Moment>,
    k: f64,
    reflection: Reflection,
    normalisation: f64,
}

impl FarField {
    pub fn new(structure: &Structure, solution: &Solution, k: f64, reflection: Reflection) -> Self {
        let mut direct = Vec::new();
        for (pulse, current) in structure.pulses.iter().zip(&solution.currents) {
            for &half in &pulse.halves {
                let (start, end) = structure.half_ends(half);
                let span = end - start;
                direct.push(Moment {
                    center: start.midpoint(end),
                    current: [current * span.x, current * span.y, current * span.z],
                });
            }
        }
        let image = if reflection.grounded() {
            direct
                .iter()
                .map(|moment| Moment {
                    center: moment.center.reflected(),
                    current: [-moment.current[0], moment.current[1], -moment.current[2]],
                })
                .collect()
        } else {
            Vec::new()
        };
        let power = solution.input_power_w().max(f64::MIN_POSITIVE);
        Self {
            direct,
            image,
            k,
            reflection,
            normalisation: FREE_SPACE_OHMS * k * k / (8.0 * std::f64::consts::PI * power),
        }
    }

    pub fn grounded(&self) -> bool {
        self.reflection.grounded()
    }

    pub fn sample(&self, direction: Direction) -> Sample {
        let unit = direction.unit();
        let (up, across) = (direction.up(), direction.across());
        let (mut vertical, mut horizontal) = self.project(&self.direct, unit, up, across);
        if self.reflection.grounded() {
            let (image_vertical, image_horizontal) = self.project(&self.image, unit, up, across);
            let (rv, rh) = self.reflection.coefficients(direction.elevation);
            vertical += rv * image_vertical;
            horizontal += rh * image_horizontal;
        }
        let v = vertical.norm_sqr();
        let h = horizontal.norm_sqr();
        let total = v + h;
        Sample {
            gain: total * self.normalisation,
            vertical_share: if total > 0.0 { v / total } else { 0.0 },
        }
    }

    fn project(
        &self,
        moments: &[Moment],
        unit: Vec3,
        up: Vec3,
        across: Vec3,
    ) -> (Complex64, Complex64) {
        let mut vertical = Complex64::new(0.0, 0.0);
        let mut horizontal = Complex64::new(0.0, 0.0);
        for moment in moments {
            let phase = Complex64::new(0.0, self.k * unit.dot(moment.center)).exp();
            let [x, y, z] = moment.current;
            vertical += (x * up.x + y * up.y + z * up.z) * phase;
            horizontal += (x * across.x + z * across.z) * phase;
        }
        (vertical, horizontal)
    }
}
