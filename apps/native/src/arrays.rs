use std::{collections::HashMap, time::SystemTime};

use sdrmm_wire::{
    array::{ArrayStatus, ProcessorStatus},
    fusion::DfFusionState,
    processor::ProcessorReading,
    ws::ServerEvent,
};

#[derive(Clone, Debug, PartialEq)]
pub struct Heard {
    pub reading: ProcessorReading,
    pub at: SystemTime,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Book {
    pub arrays: HashMap<String, ArrayStatus>,
    pub readings: HashMap<String, Heard>,
    pub fusion: HashMap<String, DfFusionState>,
}

impl Book {
    pub fn observe(&mut self, event: &ServerEvent, now: SystemTime) -> bool {
        match event {
            ServerEvent::ArrayUpdate { status } => {
                self.arrays.insert(status.node.clone(), (**status).clone());
                true
            }
            ServerEvent::ProcessorUpdate { node, reading } => {
                self.readings.insert(
                    node.clone(),
                    Heard {
                        reading: (**reading).clone(),
                        at: now,
                    },
                );
                true
            }
            ServerEvent::DfFusionUpdate { node, state } => {
                self.fusion.insert(node.clone(), (**state).clone());
                true
            }
            _ => false,
        }
    }

    pub fn seed_fusion(&mut self, node: &str, state: DfFusionState) {
        self.fusion.entry(node.to_owned()).or_insert(state);
    }

    pub fn clear_fusion(&mut self, node: &str) {
        self.fusion.remove(node);
    }

    #[must_use]
    pub fn array<'a>(&'a self, node: &str, seeded: &'a [ArrayStatus]) -> Option<&'a ArrayStatus> {
        self.arrays
            .get(node)
            .or_else(|| seeded.iter().find(|status| status.node == node))
    }
}

#[must_use]
pub fn processor_of<'a>(
    status: Option<&'a ArrayStatus>,
    node: &str,
) -> Option<&'a ProcessorStatus> {
    status?.processors.iter().find(|found| found.node == node)
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::{
        fusion::DfEstimate,
        processor::{df::DfReading, stitch::StitchReading},
    };

    use super::*;

    fn status(node: &str, center_hz: f64) -> ArrayStatus {
        ArrayStatus {
            node: node.to_owned(),
            center_hz,
            processors: vec![ProcessorStatus {
                node: "df".to_owned(),
                kind: "df".to_owned(),
                running: true,
                ..ProcessorStatus::default()
            }],
            ..ArrayStatus::default()
        }
    }

    fn df(sources: u32) -> ServerEvent {
        ServerEvent::ProcessorUpdate {
            node: "df".to_owned(),
            reading: Box::new(ProcessorReading::Df(DfReading {
                sources,
                ..DfReading::default()
            })),
        }
    }

    #[test]
    fn keeps_the_newest_reading_of_each_processor() {
        let mut book = Book::default();
        assert!(book.observe(&df(1), SystemTime::UNIX_EPOCH));
        assert!(book.observe(&df(2), SystemTime::UNIX_EPOCH));
        assert_eq!(
            book.readings["df"].reading,
            ProcessorReading::Df(DfReading {
                sources: 2,
                ..DfReading::default()
            })
        );
    }

    #[test]
    fn a_live_array_update_wins_over_the_snapshot() {
        let mut book = Book::default();
        let seeded = vec![status("bench", 100e6)];
        assert_eq!(
            book.array("bench", &seeded).map(|found| found.center_hz),
            Some(100e6)
        );
        book.observe(
            &ServerEvent::ArrayUpdate {
                status: Box::new(status("bench", 200e6)),
            },
            SystemTime::UNIX_EPOCH,
        );
        assert_eq!(
            book.array("bench", &seeded).map(|found| found.center_hz),
            Some(200e6)
        );
        assert!(book.array("other", &seeded).is_none());
    }

    #[test]
    fn a_processor_finds_its_own_health_on_its_array() {
        let held = status("bench", 1e6);
        assert!(processor_of(Some(&held), "df").is_some_and(|found| found.running));
        assert!(processor_of(Some(&held), "beam").is_none());
        assert!(processor_of(None, "df").is_none());
    }

    #[test]
    fn a_seed_never_overwrites_a_live_fusion() {
        let mut book = Book::default();
        book.observe(
            &ServerEvent::DfFusionUpdate {
                node: "grid".to_owned(),
                state: Box::new(DfFusionState {
                    samples: 4,
                    estimate: Some(DfEstimate::default()),
                    ..DfFusionState::default()
                }),
            },
            SystemTime::UNIX_EPOCH,
        );
        book.seed_fusion("grid", DfFusionState::default());
        assert_eq!(book.fusion["grid"].samples, 4);
        book.clear_fusion("grid");
        assert!(!book.fusion.contains_key("grid"));
    }

    #[test]
    fn ignores_every_other_event() {
        let mut book = Book::default();
        assert!(!book.observe(
            &ServerEvent::Hello {
                revision: 1,
                protocol: sdrmm_wire::API_PROTOCOL,
                phone: None
            },
            SystemTime::UNIX_EPOCH
        ));
        let stitch = ServerEvent::ProcessorUpdate {
            node: "wide".to_owned(),
            reading: Box::new(ProcessorReading::Stitch(StitchReading::default())),
        };
        assert!(book.observe(&stitch, SystemTime::UNIX_EPOCH));
        assert!(book.arrays.is_empty());
    }
}
