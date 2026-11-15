/// Branches of the free-running clock's table, between which it interpolates.
pub(super) const FREE_PHASES: usize = 128;
const MAX_EXACT_PHASES: usize = 512;
const MAX_EXACT_TAPS: usize = 16_384;
/// Relative error below which a whole number of input steps counts as exact.
const EXACT_TOLERANCE: f64 = 1e-9;

/// Where the next output falls: on one of a few exact branches when the ratio is a small
/// fraction, between the branches of a fine table otherwise.
#[derive(Clone, Debug)]
pub(super) enum Clock {
    Exact {
        phases: usize,
        whole: usize,
        rest: usize,
        phase: usize,
    },
    Free {
        step: f64,
        fraction: f64,
    },
}

impl Clock {
    pub(super) fn for_ratio(ratio: f64, taps_per_phase: usize) -> Self {
        let step = ratio.recip();
        let exact = (1..=MAX_EXACT_PHASES)
            .take_while(|phases| phases * taps_per_phase <= MAX_EXACT_TAPS)
            .find_map(|phases| {
                let advance = (phases as f64 * step).round();
                let exact =
                    (advance - phases as f64 * step).abs() <= EXACT_TOLERANCE * advance.max(1.0);
                (exact && advance >= 1.0).then_some((phases, advance as usize))
            });
        match exact {
            Some((phases, advance)) => Self::Exact {
                phases,
                whole: advance / phases,
                rest: advance % phases,
                phase: 0,
            },
            None => Self::Free {
                step,
                fraction: 0.0,
            },
        }
    }

    pub(super) fn phases(&self) -> usize {
        match self {
            Self::Exact { phases, .. } => *phases,
            Self::Free { .. } => FREE_PHASES,
        }
    }

    pub(super) fn reset(&mut self) {
        match self {
            Self::Exact { phase, .. } => *phase = 0,
            Self::Free { fraction, .. } => *fraction = 0.0,
        }
    }
}
