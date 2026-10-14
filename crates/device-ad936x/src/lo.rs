use std::time::Instant;

use crate::{
    apply::Write,
    fastlock::{FASTLOCK_RECALL, FASTLOCK_STORE, Step, Synth},
    iio::Context,
    layout::{FREQUENCY, RX_LO, TX_LO},
};

pub(crate) const POWERDOWN: &str = "powerdown";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Lo {
    Rx,
    Tx,
}

impl Lo {
    const fn channel(self) -> &'static str {
        match self {
            Self::Rx => RX_LO,
            Self::Tx => TX_LO,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Abilities {
    pub(crate) fastlock: bool,
    pub(crate) powerdown: bool,
}

impl Abilities {
    pub(crate) fn read(context: &Context, phy: &str) -> Self {
        let lo = |attrs: &[&str]| {
            [RX_LO, TX_LO].iter().all(|lo| {
                context
                    .device(phy)
                    .and_then(|device| device.channel(lo, true))
                    .is_some_and(|channel| attrs.iter().all(|attr| channel.has(attr)))
            })
        };
        Self {
            fastlock: lo(&[FASTLOCK_STORE, FASTLOCK_RECALL]),
            powerdown: lo(&[POWERDOWN]),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Synths {
    abilities: Abilities,
    rx: Synth,
    tx: Synth,
    external: [bool; 2],
    rx_hz: Option<u64>,
    tx_hz: Option<u64>,
    tx_on: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Resolved {
    pub(crate) writes: Vec<Write>,
    steps: Vec<(Lo, u64, Step)>,
    tx_hz: Option<u64>,
    tx_on: Option<bool>,
}

impl Synths {
    pub(crate) fn new(abilities: Abilities, tx_hz: Option<u64>, tx_on: bool) -> Self {
        Self {
            abilities,
            rx: Synth::default(),
            tx: Synth::default(),
            external: [false; 2],
            rx_hz: None,
            tx_hz,
            tx_on,
        }
    }

    pub(crate) const fn rx_hz(&self) -> Option<u64> {
        self.rx_hz
    }

    pub(crate) fn set_external(&mut self, lo: Lo, external: bool) {
        self.external[lo as usize] = external;
        self.synth(lo).forget();
    }

    pub(crate) fn forget(&mut self) {
        self.rx.forget();
        self.tx.forget();
    }

    pub(crate) fn resolve(
        &self,
        planned: Vec<Write>,
        tx_wanted: bool,
        now: Instant,
        celsius: Option<f64>,
    ) -> Resolved {
        let mut resolved = Resolved::default();
        let tx_on = !self.abilities.powerdown || tx_wanted;
        if tx_on && !self.tx_on {
            resolved.tx_on = Some(true);
            resolved.writes.push(power(true));
            let retuned = planned
                .iter()
                .any(|write| matches!(write, Write::Tune { lo: Lo::Tx, .. }));
            if let (Some(hz), false) = (self.tx_hz, retuned) {
                self.tune(Lo::Tx, hz, now, celsius, &mut resolved);
            }
        }
        for write in planned {
            match write {
                Write::Tune { lo: Lo::Tx, hz } if !tx_on => resolved.tx_hz = Some(hz),
                Write::Tune { lo, hz } => self.tune(lo, hz, now, celsius, &mut resolved),
                other => resolved.writes.push(other),
            }
        }
        if !tx_on && self.tx_on {
            resolved.tx_on = Some(false);
            resolved.writes.push(power(false));
        }
        resolved
    }

    fn tune(&self, lo: Lo, hz: u64, now: Instant, celsius: Option<f64>, resolved: &mut Resolved) {
        let step = if self.abilities.fastlock && !self.external[lo as usize] {
            self.synth_ref(lo).step(hz, now, celsius)
        } else {
            Step::Write {
                store: None,
                nudge: false,
            }
        };
        let channel = lo.channel();
        let write = |attr: &str, value: String| Write::Channel {
            output: true,
            channel: channel.to_string(),
            attr: attr.to_string(),
            value,
        };
        match step {
            Step::Recall(slot) => resolved
                .writes
                .push(write(FASTLOCK_RECALL, slot.to_string())),
            Step::Write { store, .. } => {
                for frequency in step.frequencies(hz) {
                    resolved
                        .writes
                        .push(write(FREQUENCY, frequency.to_string()));
                }
                if let Some(slot) = store {
                    resolved
                        .writes
                        .push(write(FASTLOCK_STORE, slot.to_string()));
                }
            }
        }
        resolved.steps.push((lo, hz, step));
        if lo == Lo::Tx {
            resolved.tx_hz = Some(hz);
        }
    }

    pub(crate) fn commit(&mut self, resolved: &Resolved, now: Instant, celsius: Option<f64>) {
        for (lo, hz, step) in &resolved.steps {
            if self.abilities.fastlock && !self.external[*lo as usize] {
                self.synth(*lo).commit(*hz, *step, now, celsius);
            }
            if *lo == Lo::Rx {
                self.rx_hz = Some(*hz);
            }
        }
        if let Some(hz) = resolved.tx_hz {
            self.tx_hz = Some(hz);
        }
        if let Some(on) = resolved.tx_on {
            self.tx_on = on;
            if !on {
                self.tx.forget();
            }
        }
    }

    fn synth(&mut self, lo: Lo) -> &mut Synth {
        match lo {
            Lo::Rx => &mut self.rx,
            Lo::Tx => &mut self.tx,
        }
    }

    const fn synth_ref(&self, lo: Lo) -> &Synth {
        match lo {
            Lo::Rx => &self.rx,
            Lo::Tx => &self.tx,
        }
    }
}

fn power(on: bool) -> Write {
    Write::Channel {
        output: true,
        channel: TX_LO.to_string(),
        attr: POWERDOWN.to_string(),
        value: if on { "0" } else { "1" }.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABLE: Abilities = Abilities {
        fastlock: true,
        powerdown: true,
    };

    fn tune(lo: Lo, hz: u64) -> Write {
        Write::Tune { lo, hz }
    }

    fn channel(channel: &str, attr: &str, value: &str) -> Write {
        Write::Channel {
            output: true,
            channel: channel.to_string(),
            attr: attr.to_string(),
            value: value.to_string(),
        }
    }

    fn run(synths: &mut Synths, planned: Vec<Write>, tx_wanted: bool) -> Vec<Write> {
        let now = Instant::now();
        let resolved = synths.resolve(planned, tx_wanted, now, None);
        synths.commit(&resolved, now, None);
        resolved.writes
    }

    #[test]
    fn a_silent_transmitter_is_powered_down_and_not_retuned() {
        let mut synths = Synths::new(ABLE, Some(869_000_000), true);
        let writes = run(
            &mut synths,
            vec![tune(Lo::Rx, 868_000_000), tune(Lo::Tx, 868_000_000)],
            false,
        );
        assert_eq!(
            writes,
            vec![
                channel(RX_LO, FREQUENCY, "868000000"),
                channel(RX_LO, FASTLOCK_STORE, "0"),
                channel(TX_LO, POWERDOWN, "1"),
            ]
        );
        assert!(!synths.tx_on);
        assert_eq!(synths.rx_hz(), Some(868_000_000));
    }

    #[test]
    fn a_transmitter_wanted_again_powers_up_on_the_frequency_it_missed() {
        let mut synths = Synths::new(ABLE, Some(869_000_000), false);
        run(&mut synths, vec![tune(Lo::Tx, 433_920_000)], false);
        let writes = run(&mut synths, Vec::new(), true);
        assert_eq!(
            writes,
            vec![
                channel(TX_LO, POWERDOWN, "0"),
                channel(TX_LO, FREQUENCY, "433920000"),
                channel(TX_LO, FASTLOCK_STORE, "0"),
            ]
        );
        assert!(synths.tx_on);
    }

    #[test]
    fn a_revisited_frequency_is_recalled() {
        let mut synths = Synths::new(ABLE, None, false);
        run(&mut synths, vec![tune(Lo::Rx, 868_000_000)], false);
        run(&mut synths, vec![tune(Lo::Rx, 869_000_000)], false);
        assert_eq!(
            run(&mut synths, vec![tune(Lo::Rx, 868_000_000)], false),
            vec![channel(RX_LO, FASTLOCK_RECALL, "0")]
        );
    }

    #[test]
    fn an_external_or_profileless_synthesizer_is_written_plainly() {
        let mut synths = Synths::new(ABLE, None, false);
        synths.set_external(Lo::Rx, true);
        run(&mut synths, vec![tune(Lo::Rx, 868_000_000)], false);
        assert_eq!(
            run(&mut synths, vec![tune(Lo::Rx, 868_000_000)], false),
            vec![channel(RX_LO, FREQUENCY, "868000000")]
        );
        let mut plain = Synths::new(Abilities::default(), None, true);
        assert_eq!(
            run(&mut plain, vec![tune(Lo::Tx, 868_000_000)], false),
            vec![channel(TX_LO, FREQUENCY, "868000000")],
            "a synthesizer that cannot power down stays on and is tuned"
        );
    }
}
