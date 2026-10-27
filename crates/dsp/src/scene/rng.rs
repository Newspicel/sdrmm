use num_complex::Complex;

const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;
const XORSHIFT_STAR: u64 = 0x2545_F491_4F6C_DD1D;

#[must_use]
pub fn mix(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

#[must_use]
pub fn mix_key(seed: u64, key: &str) -> u64 {
    key.bytes()
        .fold(mix(seed), |acc, byte| mix(acc ^ u64::from(byte)))
}

#[must_use]
pub fn step(value: u64) -> u64 {
    mix(value.wrapping_add(GOLDEN))
}

#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    #[must_use]
    pub fn mixed(seed: u64) -> Self {
        Self(mix(seed) | 1)
    }

    #[must_use]
    pub fn streamed(seed: u64, stream: u64) -> Self {
        let z = mix(seed ^ stream.wrapping_add(1).wrapping_mul(GOLDEN));
        Self(if z == 0 { GOLDEN } else { z })
    }

    pub fn bits(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(XORSHIFT_STAR)
    }

    pub fn uniform(&mut self) -> f64 {
        (self.bits() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn open_unit(&mut self) -> f32 {
        ((self.bits() >> 40) as f32 + 0.5) / (1u64 << 24) as f32
    }

    pub fn complex_normal(&mut self) -> Complex<f64> {
        let u = 1.0 - self.uniform();
        let v = self.uniform();
        let radius = (-u.ln()).sqrt();
        Complex::from_polar(radius, std::f64::consts::TAU * v)
    }

    pub fn normal_pair(&mut self, sigma: f32) -> Complex<f32> {
        let radius = (-2.0 * self.open_unit().ln()).sqrt() * sigma;
        let (sin, cos) = (std::f32::consts::TAU * self.open_unit()).sin_cos();
        Complex::new(radius * cos, radius * sin)
    }
}

#[derive(Clone, Debug)]
pub struct Draw(u64);

impl Draw {
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn bits(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(GOLDEN);
        mix(self.0)
    }

    pub fn unit(&mut self) -> f64 {
        (self.bits() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn within(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * self.unit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stream_seed_never_lands_on_the_dead_state() {
        for stream in 0..64u64 {
            let mut rng = Rng::streamed(0, stream);
            assert_ne!(rng.bits(), 0);
        }
        assert_ne!(Rng::mixed(0).bits(), 0);
    }

    #[test]
    fn uniform_draws_stay_inside_the_unit_interval() {
        let mut rng = Rng::mixed(7);
        for _ in 0..10_000 {
            let value = rng.uniform();
            assert!((0.0..1.0).contains(&value), "{value}");
            let open = rng.open_unit();
            assert!(open > 0.0 && open < 1.0, "{open}");
        }
    }

    #[test]
    fn normal_draws_carry_the_power_they_are_asked_for() {
        let mut rng = Rng::streamed(3, 1);
        let wide: f64 = (0..20_000)
            .map(|_| rng.complex_normal().norm_sqr())
            .sum::<f64>()
            / 20_000.0;
        assert!((wide - 1.0).abs() < 0.05, "{wide}");
        let narrow: f32 = (0..20_000)
            .map(|_| rng.normal_pair(std::f32::consts::FRAC_1_SQRT_2).norm_sqr())
            .sum::<f32>()
            / 20_000.0;
        assert!((narrow - 1.0).abs() < 0.05, "{narrow}");
    }

    #[test]
    fn a_key_changes_the_seed_it_mixes() {
        assert_ne!(mix_key(5, "kraken5"), mix_key(5, "array4"));
        assert_eq!(mix_key(5, "kraken5"), mix_key(5, "kraken5"));
        assert_ne!(step(1), step(2));
    }

    #[test]
    fn a_draw_walks_its_own_sequence_inside_the_range_it_is_given() {
        let mut draw = Draw::new(0xB1A5_ED00);
        for _ in 0..1_000 {
            let value = draw.within(-3.0, 7.0);
            assert!((-3.0..7.0).contains(&value), "{value}");
        }
        assert_ne!(Draw::new(1).bits(), Draw::new(2).bits());
    }
}
