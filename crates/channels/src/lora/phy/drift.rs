/// Running sums over the readings of one group, by symbol offset `k` and bin offset `r`.
#[derive(Clone, Copy, Debug, Default)]
struct Sums {
    count: f64,
    k: f64,
    kk: f64,
    r: f64,
    kr: f64,
    rr: f64,
}

impl Sums {
    fn add(&mut self, k: f64, r: f64) {
        self.count += 1.0;
        self.k += k;
        self.kk += k * k;
        self.r += r;
        self.kr += k * r;
        self.rr += r * r;
    }

    fn mean_k(&self) -> f64 {
        self.k / self.count
    }

    fn mean_r(&self) -> f64 {
        self.r / self.count
    }

    fn var_k(&self) -> f64 {
        if self.count > 0.0 {
            self.kk - self.k * self.k / self.count
        } else {
            0.0
        }
    }

    fn cov(&self) -> f64 {
        if self.count > 0.0 {
            self.kr - self.k * self.r / self.count
        } else {
            0.0
        }
    }

    /// The squared residuals left about this group's own line of slope `slope`.
    fn residual(&self, slope: f64) -> f64 {
        if self.count == 0.0 {
            return 0.0;
        }
        let var_r = self.rr - self.r * self.r / self.count;
        var_r - 2.0 * slope * self.cov() + slope * slope * self.var_k()
    }
}

/// What the drift is believed to be before the readings, in chips per symbol.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Prior {
    pub drift: f64,
    pub spread: f64,
}

/// The grid correction the readings call for: a frequency offset in bins and a lateness in
/// chips, both at the symbol with offset zero, and how many chips later each symbol arrives
/// than the one before.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Clock {
    pub cfo_bins: f64,
    pub late_chips: f64,
    pub drift: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Group {
    #[default]
    Up,
    Down,
    Loose,
}

#[derive(Clone, Copy, Debug, Default)]
struct Reading {
    symbol: f64,
    bins: f64,
    group: Group,
}

const MAX_READINGS: usize = 32;

/// Fits the bin offsets read on upchirps and downchirps around the SFD. A symbol `late` chips
/// late reads `cfo - late` bins on an upchirp and `cfo + late` on a downchirp, and the
/// lateness grows by the drift each symbol when the sender's chip clock is off. Loose upchirp
/// readings, from another stream whose offset is not tied to the grid, add to the drift alone.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DriftFit {
    readings: [Reading; MAX_READINGS],
    len: usize,
}

impl Default for DriftFit {
    fn default() -> Self {
        Self {
            readings: [Reading::default(); MAX_READINGS],
            len: 0,
        }
    }
}

/// Readings the fit needs beyond its unknowns before it trusts its own scatter.
const SPARE_READINGS: f64 = 2.0;
const MIN_SCATTER: f64 = 1e-4;
/// Standard errors a measured drift must stand from the prior to replace it. A drift taken from
/// noise moves the late payload symbols further than none at all.
const SIGNIFICANCE: f64 = 3.0;

/// One fit: the grid correction and the offset of the loose readings.
#[derive(Clone, Copy, Debug)]
struct Fit {
    clock: Clock,
    loose: f64,
    measured: bool,
}

impl Fit {
    fn explains(&self, reading: &Reading, outlier: f64) -> bool {
        let clock = self.clock;
        let late = clock.late_chips + clock.drift * reading.symbol;
        let expected = match reading.group {
            Group::Up => clock.cfo_bins - late,
            Group::Down => clock.cfo_bins + late,
            Group::Loose => self.loose - clock.drift * reading.symbol,
        };
        (reading.bins - expected).abs() <= outlier
    }
}

impl DriftFit {
    pub(crate) fn up(&mut self, symbol: f64, bins: f64) {
        self.push(symbol, bins, Group::Up);
    }

    pub(crate) fn down(&mut self, symbol: f64, bins: f64) {
        self.push(symbol, bins, Group::Down);
    }

    pub(crate) fn loose(&mut self, symbol: f64, bins: f64) {
        self.push(symbol, bins, Group::Loose);
    }

    fn push(&mut self, symbol: f64, bins: f64, group: Group) {
        if let Some(slot) = self.readings.get_mut(self.len) {
            *slot = Reading {
                symbol,
                bins,
                group,
            };
            self.len += 1;
        }
    }

    /// Fits once, drops every reading further than `outlier` bins from that fit, a noise peak
    /// rather than the chirp, and fits the rest. Without a drift that stands clear of the
    /// prior, only the grid readings from symbol `near` on are fitted: an unseen drift has
    /// carried them least far.
    pub(crate) fn solve(&self, prior: Prior, outlier: f64, near: f64) -> Option<Clock> {
        let first = self.fit(prior, 0.0, |_| true)?;
        let kept = |reading: &Reading| first.explains(reading, outlier);
        let second = self.fit(prior, SIGNIFICANCE, kept)?;
        if second.measured {
            return Some(second.clock);
        }
        let near = |reading: &Reading| {
            kept(reading) && reading.group != Group::Loose && reading.symbol >= near
        };
        self.fit(prior, f64::INFINITY, near).map(|fit| fit.clock)
    }

    /// The least-squares fit, with the drift pulled toward the prior as far as the readings'
    /// own scatter leaves it uncertain, and the prior kept unless the drift stands
    /// `significance` standard errors from it.
    fn fit(&self, prior: Prior, significance: f64, keep: impl Fn(&Reading) -> bool) -> Option<Fit> {
        let mut sums = [Sums::default(); 3];
        for reading in self.readings[..self.len].iter().filter(|r| keep(r)) {
            sums[reading.group as usize].add(reading.symbol, reading.bins);
        }
        let [ups, downs, loose] = sums;
        if ups.count < 1.0 || downs.count < 1.0 {
            return None;
        }
        let measured = Self::drift(&sums, prior, significance);
        let drift = measured.unwrap_or(prior.drift);
        let up = ups.mean_r() + drift * ups.mean_k();
        let down = downs.mean_r() - drift * downs.mean_k();
        let loose = if loose.count > 0.0 {
            loose.mean_r() + drift * loose.mean_k()
        } else {
            0.0
        };
        Some(Fit {
            clock: Clock {
                cfo_bins: (up + down) / 2.0,
                late_chips: (down - up) / 2.0,
                drift,
            },
            loose,
            measured: measured.is_some(),
        })
    }

    /// The drift the readings measure, if it stands `significance` standard errors from the
    /// prior. Upchirps read the drift falling and downchirps rising, each group about its own
    /// mean.
    fn drift(sums: &[Sums; 3], prior: Prior, significance: f64) -> Option<f64> {
        let [ups, downs, loose] = sums;
        let spread = ups.var_k() + downs.var_k() + loose.var_k();
        let lean = downs.cov() - ups.cov() - loose.cov();
        let readings = ups.count + downs.count + loose.count;
        let unknowns = if loose.count > 0.0 { 4.0 } else { 3.0 };
        if spread <= 0.0 || readings < unknowns + SPARE_READINGS || prior.spread <= 0.0 {
            return None;
        }
        let free = lean / spread;
        let residual = ups.residual(-free) + downs.residual(free) + loose.residual(-free);
        let scatter = (residual / (readings - unknowns)).max(MIN_SCATTER);
        let weight = scatter / (prior.spread * prior.spread);
        let drift = (lean + weight * prior.drift) / (spread + weight);
        let error = (scatter / (spread + weight)).sqrt();
        ((drift - prior.drift).abs() >= significance * error).then_some(drift)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn readings(cfo: f64, late: f64, drift: f64, noise: &[f64]) -> DriftFit {
        let mut fit = DriftFit::default();
        let mut noise = noise.iter().cycle();
        for k in -10..=-3 {
            let k = f64::from(k);
            fit.up(
                k,
                cfo - (late + drift * k) + noise.next().copied().unwrap_or(0.0),
            );
        }
        for k in 0..=1 {
            let k = f64::from(k);
            fit.down(
                k,
                cfo + late + drift * k + noise.next().copied().unwrap_or(0.0),
            );
        }
        fit
    }

    const WIDE: Prior = Prior {
        drift: 0.0,
        spread: 0.2,
    };

    #[test]
    fn clean_readings_give_back_offset_lateness_and_drift() {
        for (cfo, late, drift) in [(0.0, 0.0, 0.0), (0.3, -0.2, 0.2), (-1.1, 0.7, -0.41)] {
            let clock = readings(cfo, late, drift, &[0.0])
                .solve(WIDE, 1.0, -10.0)
                .unwrap_or_default();
            assert!((clock.cfo_bins - cfo).abs() < 1e-3, "{clock:?}");
            assert!((clock.late_chips - late).abs() < 1e-3, "{clock:?}");
            assert!((clock.drift - drift).abs() < 1e-3, "{clock:?}");
        }
    }

    const NOISE: [f64; 10] = [0.12, -0.2, 0.05, 0.17, -0.09, -0.15, 0.2, 0.01, -0.07, 0.11];

    #[test]
    fn a_drift_lost_in_the_scatter_keeps_the_prior() {
        let prior = Prior {
            drift: 0.003,
            spread: 0.2,
        };
        let clock = readings(0.1, 0.0, 0.0, &NOISE)
            .solve(prior, 1.0, -4.0)
            .unwrap_or_default();
        assert_eq!(clock.drift, 0.003);
        assert!((clock.cfo_bins - 0.1).abs() < 0.1, "{clock:?}");
    }

    #[test]
    fn a_clear_drift_replaces_the_prior() {
        let clock = readings(0.1, 0.0, 0.3, &NOISE)
            .solve(WIDE, 1.0, -10.0)
            .unwrap_or_default();
        assert!((clock.drift - 0.3).abs() < 0.05, "{clock:?}");
    }

    #[test]
    fn a_noise_peak_is_left_out() {
        let mut noise = [0.0; 10];
        noise[2] = 2.5;
        let clock = readings(-0.4, 0.2, 0.2, &noise)
            .solve(WIDE, 1.0, -10.0)
            .unwrap_or_default();
        assert!((clock.cfo_bins + 0.4).abs() < 1e-3, "{clock:?}");
        assert!((clock.drift - 0.2).abs() < 1e-3, "{clock:?}");
    }

    #[test]
    fn loose_readings_add_to_the_drift_whatever_their_offset() {
        let mut fit = DriftFit::default();
        for k in [-4.0, -3.0] {
            fit.up(k, -0.3 * k);
        }
        for k in [0.0, 1.0] {
            fit.down(k, 0.3 * k);
        }
        for k in -10..=-5 {
            let k = f64::from(k) + 0.37;
            fit.loose(k, 17.0 - 0.3 * k);
        }
        let clock = fit.solve(WIDE, 1.0, -4.0).unwrap_or_default();
        assert!((clock.drift - 0.3).abs() < 1e-3, "{clock:?}");
        assert!(clock.cfo_bins.abs() < 1e-3, "{clock:?}");
        assert!(clock.late_chips.abs() < 1e-3, "{clock:?}");
    }

    #[test]
    fn without_a_downchirp_there_is_no_fit() {
        let mut fit = DriftFit::default();
        fit.up(-4.0, 0.1);
        fit.up(-3.0, 0.2);
        assert_eq!(fit.solve(WIDE, 1.0, -10.0), None);
    }
}
