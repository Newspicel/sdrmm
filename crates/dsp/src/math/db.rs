const POWER_DB_PER_DECADE: f64 = 10.0;
const AMPLITUDE_DB_PER_DECADE: f64 = 20.0;

/// Decibel conversions for power and amplitude ratios, for `f32` and `f64` alike.
pub trait Decibels: Copy {
    #[must_use]
    fn power_to_db(self) -> Self;
    #[must_use]
    fn db_to_power(self) -> Self;
    #[must_use]
    fn amplitude_to_db(self) -> Self;
    #[must_use]
    fn db_to_amplitude(self) -> Self;
}

impl Decibels for f64 {
    fn power_to_db(self) -> Self {
        POWER_DB_PER_DECADE * self.log10()
    }

    fn db_to_power(self) -> Self {
        10f64.powf(self / POWER_DB_PER_DECADE)
    }

    fn amplitude_to_db(self) -> Self {
        AMPLITUDE_DB_PER_DECADE * self.log10()
    }

    fn db_to_amplitude(self) -> Self {
        10f64.powf(self / AMPLITUDE_DB_PER_DECADE)
    }
}

impl Decibels for f32 {
    fn power_to_db(self) -> Self {
        POWER_DB_PER_DECADE as Self * self.log10()
    }

    fn db_to_power(self) -> Self {
        10f32.powf(self / POWER_DB_PER_DECADE as Self)
    }

    fn amplitude_to_db(self) -> Self {
        AMPLITUDE_DB_PER_DECADE as Self * self.log10()
    }

    fn db_to_amplitude(self) -> Self {
        10f32.powf(self / AMPLITUDE_DB_PER_DECADE as Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_and_amplitude_round_trip() {
        assert!((100.0f64.power_to_db() - 20.0).abs() < 1e-12);
        assert!((100.0f64.amplitude_to_db() - 40.0).abs() < 1e-12);
        assert!((20.0f32.db_to_power() - 100.0).abs() < 1e-3);
        assert!((40.0f32.db_to_amplitude() - 100.0).abs() < 1e-3);
    }
}
