use std::collections::BTreeMap;

use sdrmm_wire::{ClockBank, NoiseSource};

use crate::dongle::Listing;

mod control;

#[cfg(test)]
mod hardware;

pub(crate) use control::{Pins, control, hold_control};

const FIRST_SERIAL: u32 = 1000;
const KRAKEN_LANES: u32 = 5;
const KERBEROS_LANES: u32 = 4;

pub(crate) const KRAKEN_HUB: (u16, u16) = (0x0424, 0x2517);
pub(crate) const NOISE_SOURCE_PIN: u8 = 0;
pub(crate) const MAX_RATE_HZ: f64 = 2_560_000.0;

pub(crate) const fn bias_tee_pin(lane: u32) -> u8 {
    lane as u8 + 1
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Model {
    Kraken,
    Kerberos,
}

impl Model {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Kraken => "KrakenSDR",
            Self::Kerberos => "KerberosSDR",
        }
    }

    pub(crate) const fn lanes(self) -> u32 {
        match self {
            Self::Kraken => KRAKEN_LANES,
            Self::Kerberos => KERBEROS_LANES,
        }
    }

    pub(crate) const fn noise_source(self) -> NoiseSource {
        match self {
            Self::Kraken => NoiseSource::Isolated,
            Self::Kerberos => NoiseSource::Unisolated,
        }
    }

    pub(crate) const fn has_bias_tees(self) -> bool {
        matches!(self, Self::Kraken)
    }

    fn identify(hub: Option<(u16, u16)>, present: &[u32]) -> Option<Self> {
        let past_kerberos =
            present.len() >= KERBEROS_LANES as usize && present.contains(&(KRAKEN_LANES - 1));
        if hub == Some(KRAKEN_HUB) || past_kerberos {
            return Some(Self::Kraken);
        }
        present
            .iter()
            .copied()
            .eq(0..KERBEROS_LANES)
            .then_some(Self::Kerberos)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Lane {
    pub(crate) model: Model,
    pub(crate) id: String,
    pub(crate) lane: u32,
}

impl Lane {
    pub(crate) fn label(&self) -> String {
        format!("{} {} lane {}", self.model.name(), self.id, self.lane + 1)
    }

    pub(crate) const fn controls(&self) -> bool {
        self.lane == 0
    }

    pub(crate) fn wire(&self) -> ClockBank {
        ClockBank {
            id: self.id.clone(),
            lane: self.lane,
            lanes: self.model.lanes(),
        }
    }
}

fn lane_of(descriptor: &Listing) -> Option<u32> {
    let serial: u32 = descriptor.serial.as_deref()?.parse().ok()?;
    let lane = serial.checked_sub(FIRST_SERIAL)?;
    (lane < KRAKEN_LANES).then_some(lane)
}

fn hub_key(descriptor: &Listing) -> String {
    let ports = descriptor
        .port_chain
        .split_last()
        .map_or(&[][..], |(_, above)| above);
    if ports.is_empty() {
        return descriptor.bus.clone();
    }
    let path: Vec<String> = ports.iter().map(u8::to_string).collect();
    format!("{}/{}", descriptor.bus, path.join("."))
}

#[derive(Default)]
struct Behind {
    hub: Option<(u16, u16)>,
    lanes: Vec<(u32, usize)>,
}

fn model_behind(behind: &Behind) -> Option<Model> {
    let mut present: Vec<u32> = behind.lanes.iter().map(|(lane, _)| *lane).collect();
    present.sort_unstable();
    if present.windows(2).any(|pair| pair[0] == pair[1]) {
        return None;
    }
    Model::identify(behind.hub, &present)
}

pub(crate) fn lanes(descriptors: &[Listing]) -> Vec<Option<Lane>> {
    let mut behind: BTreeMap<String, Behind> = BTreeMap::new();
    for (index, descriptor) in descriptors.iter().enumerate() {
        if let Some(lane) = lane_of(descriptor) {
            let group = behind.entry(hub_key(descriptor)).or_default();
            group.hub = group.hub.or(descriptor.hub);
            group.lanes.push((lane, index));
        }
    }
    let mut found: Vec<Option<Lane>> = descriptors.iter().map(|_| None).collect();
    for (id, group) in behind {
        let Some(model) = model_behind(&group) else {
            continue;
        };
        for (lane, index) in group.lanes {
            if let Some(slot) = found.get_mut(index) {
                *slot = Some(Lane {
                    model,
                    id: id.clone(),
                    lane,
                });
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dongle::Board;

    const OTHER_HUB: (u16, u16) = (0x05e3, 0x0610);

    fn dongle(bus: &str, chain: &[u8], serial: Option<&str>) -> Listing {
        Listing {
            index: 0,
            bus: bus.to_owned(),
            address: chain.last().copied().unwrap_or(1),
            manufacturer: None,
            product: None,
            serial: serial.map(str::to_owned),
            port_chain: chain.to_vec(),
            board: Board::Generic,
            hub: None,
        }
    }

    fn lanes_behind(bus: &str, hub: u8, id: Option<(u16, u16)>, lanes: &[u32]) -> Vec<Listing> {
        lanes
            .iter()
            .map(|lane| Listing {
                hub: id,
                ..dongle(
                    bus,
                    &[hub, *lane as u8 + 1],
                    Some(&(FIRST_SERIAL + lane).to_string()),
                )
            })
            .collect()
    }

    fn bank_behind(bus: &str, hub: u8, count: u32) -> Vec<Listing> {
        lanes_behind(bus, hub, Some(KRAKEN_HUB), &(0..count).collect::<Vec<_>>())
    }

    fn found(descriptors: &[Listing]) -> Vec<Lane> {
        lanes(descriptors).into_iter().flatten().collect()
    }

    #[test]
    fn five_chains_behind_one_hub_are_lanes_of_one_bank() {
        let found = found(&bank_behind("0", 3, 5));
        assert_eq!(found.len(), 5);
        assert!(found.iter().all(|lane| lane.model == Model::Kraken));
        assert!(found.iter().all(|lane| lane.id == "0/3"));
        assert_eq!(
            found.iter().map(|lane| lane.lane).collect::<Vec<_>>(),
            [0, 1, 2, 3, 4]
        );
        assert_eq!(found[0].label(), "KrakenSDR 0/3 lane 1");
        assert_eq!(found[4].label(), "KrakenSDR 0/3 lane 5");
        assert_eq!(
            found[2].wire(),
            ClockBank {
                id: "0/3".to_owned(),
                lane: 2,
                lanes: 5,
            }
        );
    }

    #[test]
    fn the_first_lane_carries_the_switches_of_the_whole_bank() {
        let found = found(&bank_behind("0", 3, 5));
        assert!(found[0].controls());
        assert!(found[1..].iter().all(|lane| !lane.controls()));
        assert_eq!(
            found
                .iter()
                .map(|lane| bias_tee_pin(lane.lane))
                .collect::<Vec<_>>(),
            [1, 2, 3, 4, 5]
        );
        assert_ne!(bias_tee_pin(0), NOISE_SOURCE_PIN);
        assert_eq!(Model::Kraken.noise_source(), NoiseSource::Isolated);
        assert_eq!(Model::Kerberos.noise_source(), NoiseSource::Unisolated);
        assert!(Model::Kraken.has_bias_tees());
        assert!(!Model::Kerberos.has_bias_tees());
    }

    #[test]
    fn five_serials_behind_an_unknown_hub_are_a_kraken() {
        let found = found(&lanes_behind("0", 3, None, &[0, 1, 2, 3, 4]));
        assert_eq!(found.len(), 5);
        assert!(found.iter().all(|lane| lane.model == Model::Kraken));
    }

    #[test]
    fn kerberos_needs_a_non_kraken_hub() {
        let kerberos = found(&lanes_behind("0", 3, Some(OTHER_HUB), &[0, 1, 2, 3]));
        assert_eq!(kerberos.len(), 4);
        assert!(kerberos.iter().all(|lane| lane.model == Model::Kerberos));
        assert_eq!(kerberos[0].label(), "KerberosSDR 0/3 lane 1");
        assert_eq!(kerberos[0].wire().lanes, 4);
        let unknown = found(&lanes_behind("0", 3, None, &[0, 1, 2, 3]));
        assert_eq!(unknown[0].model, Model::Kerberos);
        let kraken = found(&lanes_behind("0", 3, Some(KRAKEN_HUB), &[0, 1, 2, 3]));
        assert_eq!(kraken[0].model, Model::Kraken);
    }

    #[test]
    fn an_incomplete_kraken_still_names_five_lanes() {
        let found = found(&bank_behind("0", 3, 4));
        assert_eq!(found.len(), 4);
        assert!(found.iter().all(|lane| lane.wire().lanes == 5));
        assert!(found.iter().all(|lane| lane.model == Model::Kraken));
    }

    #[test]
    fn lanes_are_numbered_by_serial_rather_than_by_enumeration_order() {
        let mut descriptors = bank_behind("0", 3, 5);
        descriptors.reverse();
        let found = found(&descriptors);
        assert_eq!(
            found.iter().map(|lane| lane.lane).collect::<Vec<_>>(),
            [4, 3, 2, 1, 0]
        );
    }

    #[test]
    fn a_missing_chain_leaves_the_others_on_their_own_lanes() {
        let mut descriptors = lanes_behind("0", 3, None, &[0, 1, 2, 3, 4]);
        descriptors.remove(2);
        let found = found(&descriptors);
        assert_eq!(
            found.iter().map(|lane| lane.lane).collect::<Vec<_>>(),
            [0, 1, 3, 4]
        );
        assert!(found[0].controls());
    }

    #[test]
    fn a_few_kraken_serials_behind_another_hub_stay_loose() {
        assert!(found(&lanes_behind("0", 3, Some(OTHER_HUB), &[0, 1])).is_empty());
        assert!(found(&lanes_behind("0", 3, Some(OTHER_HUB), &[4])).is_empty());
        assert!(found(&lanes_behind("0", 3, None, &[2, 3, 4])).is_empty());
    }

    #[test]
    fn ordinary_dongles_are_never_lanes() {
        let descriptors = vec![
            dongle("0", &[1], Some("00000001")),
            dongle("0", &[2], Some("77771111")),
            dongle("0", &[3], None),
        ];
        assert!(found(&descriptors).is_empty());
        assert_eq!(lanes(&descriptors).len(), 3);
    }

    #[test]
    fn two_banks_on_one_machine_are_told_apart_by_where_they_hang() {
        let mut descriptors = bank_behind("0", 3, 5);
        descriptors.extend(bank_behind("0", 7, 5));
        let found = found(&descriptors);
        assert_eq!(found.len(), 10);
        assert!(found[..5].iter().all(|lane| lane.id == "0/3"));
        assert!(found[5..].iter().all(|lane| lane.id == "0/7"));
    }

    #[test]
    fn two_banks_that_cannot_be_told_apart_are_not_grouped() {
        let descriptors: Vec<Listing> = (0..10)
            .map(|lane| dongle("0", &[], Some(&(FIRST_SERIAL + lane % 5).to_string())))
            .collect();
        assert!(found(&descriptors).is_empty());
    }

    #[test]
    fn a_bus_that_reports_no_topology_still_groups_one_bank() {
        let descriptors: Vec<Listing> = (0..5)
            .map(|lane| dongle("0", &[], Some(&(FIRST_SERIAL + lane).to_string())))
            .collect();
        let found = found(&descriptors);
        assert_eq!(found.len(), 5);
        assert!(found.iter().all(|lane| lane.id == "0"));
    }
}
