use std::collections::BTreeSet;

use sdrmm_wire::{Capabilities, Coherence, NoiseSource};

use super::LaneRef;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TierDecision {
    pub(crate) tier: Coherence,
    pub(crate) devices: usize,
    pub(crate) keeps_phase: bool,
    pub(crate) structural_zero_delay: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Clock<'a> {
    Bank(&'a str),
    Device(u32),
}

fn clock<'a>(lane: &LaneRef, caps: &'a Capabilities) -> Clock<'a> {
    caps.bank
        .as_ref()
        .map_or(Clock::Device(lane.device_set), |bank| {
            Clock::Bank(bank.id.as_str())
        })
}

pub(crate) fn decide(
    lanes: &[(LaneRef, &Capabilities)],
    declared: Coherence,
    drift_failed: bool,
) -> TierDecision {
    let devices: BTreeSet<u32> = lanes.iter().map(|(lane, _)| lane.device_set).collect();
    let clocks: BTreeSet<Clock<'_>> = lanes.iter().map(|(lane, caps)| clock(lane, caps)).collect();
    let keeps_phase = !lanes.is_empty() && lanes.iter().all(|(_, caps)| caps.retune_keeps_phase);
    let replayed = !lanes.is_empty()
        && lanes
            .iter()
            .all(|(_, caps)| caps.noise_source == NoiseSource::Replayed);
    let tier = if clocks.len() == 1 {
        lanes
            .first()
            .map_or(Coherence::None, |(_, caps)| caps.coherence)
    } else {
        clocks
            .iter()
            .filter_map(|held| {
                let mut members = lanes
                    .iter()
                    .filter(|(lane, caps)| clock(lane, caps) == *held);
                let (_, caps) = members.next()?;
                members.next().map(|_| caps.coherence)
            })
            .fold(declared, Coherence::min)
    };
    let tier = if drift_failed { Coherence::None } else { tier };
    TierDecision {
        tier,
        devices: devices.len(),
        keeps_phase,
        structural_zero_delay: devices.len() == 1
            && (tier == Coherence::PhaseCoherent || (replayed && tier != Coherence::None)),
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::{Agc, ClockBank, DcArtifact, Duplex, StreamScope};

    use super::*;

    fn caps(coherence: Coherence, retune_keeps_phase: bool) -> Capabilities {
        Capabilities {
            freq_ranges: Vec::new(),
            sample_rates: Vec::new(),
            sample_rate_ranges: Vec::new(),
            gains: Vec::new(),
            antennas: Vec::new(),
            bandwidths: Vec::new(),
            bandwidth_ranges: Vec::new(),
            bandwidth_auto: false,
            bias_tee: false,
            agc: Agc::None,
            extra: Vec::new(),
            ppm: false,
            duplex: Duplex::RxOnly,
            rx_streams: 5,
            tx_streams: 0,
            per_stream: StreamScope::default(),
            directional: None,
            dc_artifact: DcArtifact::Managed,
            hardware_sweep: false,
            coherence,
            noise_source: NoiseSource::Isolated,
            retune_keeps_phase,
            rx_inputs: Vec::new(),
            clean_fraction: None,
            bank: None,
        }
    }

    fn banked(id: &str, lane: u32, lanes: u32) -> Capabilities {
        Capabilities {
            rx_streams: 1,
            bank: Some(ClockBank {
                id: id.to_owned(),
                lane,
                lanes,
            }),
            ..caps(Coherence::TimeSync, false)
        }
    }

    const fn lane(device_set: u32, stream: u32) -> LaneRef {
        LaneRef { device_set, stream }
    }

    #[test]
    fn one_device_takes_the_driver_tier() {
        let kraken = caps(Coherence::TimeSync, false);
        let lanes: Vec<_> = (0..5).map(|stream| (lane(1, stream), &kraken)).collect();
        let decision = decide(&lanes, Coherence::PhaseCoherent, false);
        assert_eq!(decision.tier, Coherence::TimeSync);
        assert_eq!(decision.devices, 1);
        assert!(!decision.structural_zero_delay);
        let coherent = caps(Coherence::PhaseCoherent, true);
        let lanes = [(lane(2, 0), &coherent), (lane(2, 1), &coherent)];
        let decision = decide(&lanes, Coherence::None, false);
        assert_eq!(decision.tier, Coherence::PhaseCoherent);
        assert!(decision.structural_zero_delay);
        assert!(decision.keeps_phase);
    }

    #[test]
    fn several_devices_take_the_declared_tier_capped_by_multi_lane_members() {
        let dongle = caps(Coherence::None, false);
        let pair = caps(Coherence::TimeSync, false);
        let singles = [(lane(1, 0), &dongle), (lane(2, 0), &dongle)];
        let decision = decide(&singles, Coherence::PhaseCoherent, false);
        assert_eq!(decision.tier, Coherence::PhaseCoherent);
        assert_eq!(decision.devices, 2);
        assert!(!decision.structural_zero_delay);
        let mixed = [
            (lane(1, 0), &dongle),
            (lane(3, 0), &pair),
            (lane(3, 1), &pair),
        ];
        assert_eq!(
            decide(&mixed, Coherence::PhaseCoherent, false).tier,
            Coherence::TimeSync
        );
        assert_eq!(decide(&mixed, Coherence::None, false).tier, Coherence::None);
    }

    #[test]
    fn a_replayed_collection_starts_lined_up() {
        let replayed = Capabilities {
            noise_source: NoiseSource::Replayed,
            ..caps(Coherence::TimeSync, false)
        };
        let lanes: Vec<_> = (0..5).map(|stream| (lane(4, stream), &replayed)).collect();
        let decision = decide(&lanes, Coherence::None, false);
        assert_eq!(decision.tier, Coherence::TimeSync);
        assert!(decision.structural_zero_delay);
        assert!(!decide(&lanes, Coherence::None, true).structural_zero_delay);
        let live = caps(Coherence::TimeSync, false);
        let mixed = [(lane(4, 0), &replayed), (lane(4, 1), &live)];
        assert!(!decide(&mixed, Coherence::None, false).structural_zero_delay);
    }

    #[test]
    fn dongles_on_one_bank_clock_take_the_tier_the_bank_gives() {
        let held: Vec<Capabilities> = (0..5).map(|lane| banked("001/4", lane, 5)).collect();
        let lanes: Vec<(LaneRef, &Capabilities)> = held
            .iter()
            .enumerate()
            .map(|(index, caps)| (lane(index as u32 + 1, 0), caps))
            .collect();
        let decision = decide(&lanes, Coherence::None, false);
        assert_eq!(
            decision.tier,
            Coherence::TimeSync,
            "the shared clock is the hardware's word, not the operator's"
        );
        assert_eq!(decision.devices, 5);
        assert!(
            !decision.structural_zero_delay,
            "five starts of their own line up by measurement"
        );
        assert_eq!(decide(&lanes, Coherence::None, true).tier, Coherence::None);
    }

    #[test]
    fn a_loose_dongle_beside_a_bank_leaves_the_tier_to_the_operator() {
        let held: Vec<Capabilities> = (0..5).map(|lane| banked("001/4", lane, 5)).collect();
        let loose = caps(Coherence::None, false);
        let mut lanes: Vec<(LaneRef, &Capabilities)> = held
            .iter()
            .enumerate()
            .map(|(index, caps)| (lane(index as u32 + 1, 0), caps))
            .collect();
        lanes.push((lane(9, 0), &loose));
        assert_eq!(
            decide(&lanes, Coherence::TimeSync, false).tier,
            Coherence::TimeSync
        );
        assert_eq!(decide(&lanes, Coherence::None, false).tier, Coherence::None);
    }

    #[test]
    fn two_banks_in_one_array_are_capped_by_what_is_declared() {
        let first: Vec<Capabilities> = (0..5).map(|lane| banked("001/4", lane, 5)).collect();
        let second: Vec<Capabilities> = (0..5).map(|lane| banked("001/9", lane, 5)).collect();
        let lanes: Vec<(LaneRef, &Capabilities)> = first
            .iter()
            .chain(&second)
            .enumerate()
            .map(|(index, caps)| (lane(index as u32 + 1, 0), caps))
            .collect();
        assert_eq!(
            decide(&lanes, Coherence::TimeSync, false).tier,
            Coherence::TimeSync
        );
        assert_eq!(decide(&lanes, Coherence::None, false).tier, Coherence::None);
    }

    #[test]
    fn measured_drift_caps_the_tier_to_none() {
        let coherent = caps(Coherence::PhaseCoherent, true);
        let lanes = [(lane(1, 0), &coherent), (lane(1, 1), &coherent)];
        let decision = decide(&lanes, Coherence::PhaseCoherent, true);
        assert_eq!(decision.tier, Coherence::None);
        assert!(!decision.structural_zero_delay);
    }
}
