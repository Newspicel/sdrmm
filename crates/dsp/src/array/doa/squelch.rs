use super::SquelchConfig;

/// Opens when the eigenvalue ratio reaches `open_db` and closes once it falls a hysteresis below.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Squelch {
    open: bool,
}

impl Squelch {
    #[must_use]
    pub const fn new() -> Self {
        Self { open: false }
    }

    pub const fn reset(&mut self) {
        self.open = false;
    }

    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.open
    }

    pub fn update(&mut self, config: Option<SquelchConfig>, ratio_db: f32) -> bool {
        self.open = match config {
            None => true,
            Some(config) if self.open => ratio_db >= config.open_db - config.hysteresis_db,
            Some(config) => ratio_db >= config.open_db,
        };
        self.open
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn squelch_opens_above_threshold_and_closes_below_hysteresis() {
        let config = Some(SquelchConfig {
            open_db: 6.0,
            hysteresis_db: 1.0,
        });
        let mut squelch = Squelch::new();
        assert!(!squelch.update(config, 5.9));
        assert!(squelch.update(config, 6.0));
        assert!(squelch.update(config, 5.5));
        assert!(!squelch.update(config, 4.9));
        assert!(!squelch.update(config, 5.5));
        assert!(!squelch.update(config, f32::NAN));
        assert!(squelch.update(None, -40.0));
        assert!(squelch.is_open());
        squelch.reset();
        assert!(!squelch.is_open());
    }
}
