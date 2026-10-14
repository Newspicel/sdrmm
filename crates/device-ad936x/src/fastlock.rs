use std::time::{Duration, Instant};

pub(crate) const FASTLOCK_STORE: &str = "fastlock_store";
pub(crate) const FASTLOCK_RECALL: &str = "fastlock_recall";
pub(crate) const SLOTS: usize = 8;

const IDLE: Duration = Duration::from_secs(2);
const WINDOW_HZ: u64 = 100_000_000;
const DRIFT_CELSIUS: f64 = 10.0;
const NUDGE_HZ: u64 = 1_000;
const GAIN_TABLE_EDGES_HZ: [u64; 2] = [1_300_000_000, 4_000_000_000];

#[derive(Clone, Copy, Debug, PartialEq)]
struct Slot {
    hz: u64,
    used: Instant,
    celsius: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Recall(usize),
    Write { store: Option<usize>, nudge: bool },
}

impl Step {
    pub(crate) fn frequencies(self, hz: u64) -> Vec<u64> {
        match self {
            Self::Recall(_) => Vec::new(),
            Self::Write { nudge: true, .. } => vec![hz + NUDGE_HZ, hz],
            Self::Write { nudge: false, .. } => vec![hz],
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Synth {
    slots: [Option<Slot>; SLOTS],
    written: Option<u64>,
    recalled: bool,
}

impl Synth {
    pub(crate) fn step(&self, hz: u64, now: Instant, celsius: Option<f64>) -> Step {
        if let Some(slot) = self.slot_of(hz, celsius) {
            return Step::Recall(slot);
        }
        Step::Write {
            store: self.free_slot(hz, now, celsius),
            nudge: self.recalled && self.written == Some(hz),
        }
    }

    pub(crate) fn commit(&mut self, hz: u64, step: Step, now: Instant, celsius: Option<f64>) {
        match step {
            Step::Recall(slot) => {
                if let Some(held) = self.slots[slot].as_mut() {
                    held.used = now;
                }
                self.recalled = true;
            }
            Step::Write { store, .. } => {
                self.written = Some(hz);
                self.recalled = false;
                if let Some(slot) = store {
                    self.slots[slot] = Some(Slot {
                        hz,
                        used: now,
                        celsius,
                    });
                }
            }
        }
    }

    pub(crate) fn forget(&mut self) {
        self.slots = [None; SLOTS];
        self.recalled = true;
    }

    fn slot_of(&self, hz: u64, celsius: Option<f64>) -> Option<usize> {
        let written = self.written?;
        self.slots.iter().position(|slot| {
            slot.is_some_and(|slot| slot.hz == hz && usable(slot, written, celsius))
        })
    }

    fn free_slot(&self, hz: u64, now: Instant, celsius: Option<f64>) -> Option<usize> {
        let same = self
            .slots
            .iter()
            .position(|slot| slot.is_some_and(|slot| slot.hz == hz));
        same.or_else(|| self.slots.iter().position(Option::is_none))
            .or_else(|| {
                self.slots
                    .iter()
                    .position(|slot| slot.is_some_and(|slot| !usable(slot, hz, celsius)))
            })
            .or_else(|| {
                self.slots
                    .iter()
                    .enumerate()
                    .filter_map(|(index, slot)| slot.map(|slot| (index, slot.used)))
                    .filter(|(_, used)| now.saturating_duration_since(*used) >= IDLE)
                    .min_by_key(|(_, used)| *used)
                    .map(|(index, _)| index)
            })
    }
}

fn usable(slot: Slot, written: u64, celsius: Option<f64>) -> bool {
    slot.hz.abs_diff(written) <= WINDOW_HZ
        && gain_table(slot.hz) == gain_table(written)
        && match (slot.celsius, celsius) {
            (Some(then), Some(now)) => (then - now).abs() <= DRIFT_CELSIUS,
            _ => true,
        }
}

fn gain_table(hz: u64) -> usize {
    GAIN_TABLE_EDGES_HZ
        .iter()
        .filter(|edge| hz >= **edge)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MHZ: u64 = 1_000_000;

    fn visit(synth: &mut Synth, hz: u64, now: Instant) -> Step {
        let step = synth.step(hz, now, Some(40.0));
        synth.commit(hz, step, now, Some(40.0));
        step
    }

    #[test]
    fn a_frequency_is_written_once_and_recalled_after() {
        let mut synth = Synth::default();
        let now = Instant::now();
        assert_eq!(
            visit(&mut synth, 868 * MHZ, now),
            Step::Write {
                store: Some(0),
                nudge: false
            }
        );
        assert_eq!(
            visit(&mut synth, 869 * MHZ, now),
            Step::Write {
                store: Some(1),
                nudge: false
            }
        );
        assert_eq!(visit(&mut synth, 868 * MHZ, now), Step::Recall(0));
        assert_eq!(visit(&mut synth, 869 * MHZ, now), Step::Recall(1));
    }

    #[test]
    fn a_full_bank_keeps_what_was_recalled_lately_and_writes_the_rest_plainly() {
        let mut synth = Synth::default();
        let now = Instant::now();
        for channel in 0..SLOTS as u64 {
            visit(&mut synth, (860 + channel) * MHZ, now);
        }
        assert_eq!(
            visit(&mut synth, 870 * MHZ, now),
            Step::Write {
                store: None,
                nudge: false
            },
            "a sweep wider than the bank does not thrash it"
        );
        assert_eq!(visit(&mut synth, 860 * MHZ, now), Step::Recall(0));
        let later = now + IDLE;
        assert!(matches!(
            visit(&mut synth, 871 * MHZ, later),
            Step::Write { store: Some(_), .. }
        ));
    }

    #[test]
    fn a_profile_far_from_the_last_written_one_or_past_a_gain_table_edge_is_not_recalled() {
        let mut synth = Synth::default();
        let now = Instant::now();
        visit(&mut synth, 1_250 * MHZ, now);
        visit(&mut synth, 1_310 * MHZ, now);
        assert!(
            matches!(visit(&mut synth, 1_250 * MHZ, now), Step::Write { .. }),
            "the kernel loads another gain table below 1.3 GHz"
        );
        visit(&mut synth, 400 * MHZ, now);
        visit(&mut synth, 600 * MHZ, now);
        assert!(matches!(
            visit(&mut synth, 400 * MHZ, now),
            Step::Write { .. }
        ));
    }

    #[test]
    fn a_profile_stored_at_another_temperature_is_written_again() {
        let mut synth = Synth::default();
        let now = Instant::now();
        let step = synth.step(868 * MHZ, now, Some(30.0));
        synth.commit(868 * MHZ, step, now, Some(30.0));
        assert!(matches!(
            synth.step(868 * MHZ, now, Some(45.0)),
            Step::Write { store: Some(0), .. }
        ));
        assert_eq!(synth.step(868 * MHZ, now, Some(35.0)), Step::Recall(0));
    }

    #[test]
    fn the_last_written_frequency_is_nudged_when_a_recall_moved_the_synthesizer_away() {
        let mut synth = Synth::default();
        let now = Instant::now();
        visit(&mut synth, 868 * MHZ, now);
        visit(&mut synth, 869 * MHZ, now);
        visit(&mut synth, 868 * MHZ, now);
        synth.forget();
        let step = synth.step(869 * MHZ, now, Some(40.0));
        assert_eq!(
            step,
            Step::Write {
                store: Some(0),
                nudge: true
            }
        );
        assert_eq!(
            step.frequencies(869 * MHZ),
            vec![869 * MHZ + NUDGE_HZ, 869 * MHZ]
        );
    }

    #[test]
    fn nothing_is_recalled_before_a_frequency_was_written() {
        let synth = Synth::default();
        assert_eq!(
            synth.step(868 * MHZ, Instant::now(), None),
            Step::Write {
                store: Some(0),
                nudge: false
            }
        );
    }
}
