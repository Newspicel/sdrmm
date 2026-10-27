use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use sdrmm_channels::{ChannelError, array_processor::MAX_LANES};
use sdrmm_wire::{
    ArrayFailure, ArrayGain, ArrayTune, ArrayTuningMode, Capabilities, Coherence, DcArtifact,
    DeviceSetStatus, DeviceSettings, GeometryError, NoiseSource,
    array::{MAX_ARRAY_GAIN_DB, MAX_ARRAY_LANES, MIN_ARRAY_GAIN_DB, MIN_ARRAY_LANES},
};

use super::Lease;
use crate::{
    DEFAULT_CENTER_HZ, DEFAULT_SAMPLE_RATE, EngineError, Inner,
    array::{
        ArrayShape, ArraySpec, LaneFeed, LaneRef, LiveFrame, NoiseSwitch, TapPort, TierDecision,
        tier, tuner,
    },
    center_of, lock_runtime, sample_rate_of,
};

pub(super) struct Member {
    pub(super) slot: usize,
    pub(super) lane: LaneRef,
    pub(super) caps: Capabilities,
    pub(super) settings: DeviceSettings,
    pub(super) running: bool,
    pub(super) port: Option<Arc<TapPort>>,
    pub(super) in_flight: u64,
    pub(super) streams: u32,
}

impl Member {
    pub(super) fn rate(&self) -> f64 {
        sample_rate_of(&self.settings)
    }

    pub(super) fn center(&self) -> f64 {
        center_of(&self.settings, self.lane.stream, &self.caps.per_stream)
    }
}

pub(super) type Leased = (Vec<Option<LaneFeed>>, Vec<Option<Lease>>);

pub(super) struct Survey {
    pub(super) lanes: Vec<Option<LaneRef>>,
    pub(super) members: Vec<Member>,
    pub(super) positions: Vec<[f64; 3]>,
}

fn geometry_refusal(error: GeometryError) -> EngineError {
    match error {
        GeometryError::CountMismatch { positions, lanes } => {
            ArrayFailure::GeometryMismatch { positions, lanes }.into()
        }
        GeometryError::TooManyLanes => ArrayFailure::TooManyLanes.into(),
        GeometryError::TooFewLanes => ArrayFailure::Unwired.into(),
        GeometryError::OutOfRange | GeometryError::NotFinite => {
            ChannelError::Refused("Positions out of range").into()
        }
    }
}

fn check_shape(spec: &ArraySpec) -> Result<Vec<[f64; 3]>, EngineError> {
    let count = spec.lanes.len();
    if count > MAX_ARRAY_LANES as usize {
        return Err(ArrayFailure::TooManyLanes.into());
    }
    if count < MIN_ARRAY_LANES as usize {
        return Err(ArrayFailure::Unwired.into());
    }
    if let Some(problem) = spec.settings.problem() {
        return Err(ChannelError::Refused(problem).into());
    }
    for (slot, lane) in spec.lanes.iter().enumerate() {
        if lane.is_some() && spec.lanes[..slot].contains(lane) {
            return Err(ArrayFailure::DuplicateLane { lane: slot as u32 }.into());
        }
    }
    spec.settings
        .geometry
        .positions(count)
        .map_err(geometry_refusal)
}

fn member(
    inner: &Inner,
    spec: &ArraySpec,
    slot: usize,
    lane: LaneRef,
) -> Result<Member, EngineError> {
    let state = inner
        .device_sets
        .get(&lane.device_set)
        .ok_or(EngineError::DeviceSetNotFound(lane.device_set))?;
    if lane.stream >= state.physical_streams() {
        return Err(EngineError::StreamOutOfRange {
            stream: lane.stream,
            streams: state.physical_streams(),
        });
    }
    if let Some(by) = state.held.get(&lane.stream)
        && *by != spec.node
    {
        return Err(ArrayFailure::LaneHeld {
            lane: slot as u32,
            by: by.clone(),
        }
        .into());
    }
    let runtime = lock_runtime(&state.runtime);
    Ok(Member {
        slot,
        lane,
        caps: state.capabilities.clone(),
        settings: state.settings.clone(),
        running: state.status == DeviceSetStatus::Running,
        port: runtime.tap_ports().get(lane.stream as usize).cloned(),
        in_flight: runtime.in_flight_samples(),
        streams: state.physical_streams(),
    })
}

pub(super) fn survey(inner: &Inner, spec: &ArraySpec) -> Result<Survey, EngineError> {
    let positions = check_shape(spec)?;
    let members = spec
        .lanes
        .iter()
        .enumerate()
        .filter_map(|(slot, lane)| lane.map(|lane| (slot, lane)))
        .map(|(slot, lane)| member(inner, spec, slot, lane))
        .collect::<Result<Vec<_>, EngineError>>()?;
    Ok(Survey {
        lanes: spec.lanes.clone(),
        members,
        positions,
    })
}

impl Survey {
    pub(super) fn anchor(&self) -> Option<u32> {
        self.members.first().map(|member| member.lane.device_set)
    }

    pub(super) fn rate(&self) -> f64 {
        self.members
            .first()
            .map_or(DEFAULT_SAMPLE_RATE, Member::rate)
    }

    pub(super) fn usable(&self, member: &Member) -> bool {
        member.running && member.port.is_some() && member.rate() == self.rate()
    }

    pub(super) fn device_sets(&self) -> BTreeSet<u32> {
        self.members
            .iter()
            .map(|member| member.lane.device_set)
            .collect()
    }

    pub(super) fn tier(&self, declared: Coherence, drift_failed: bool) -> TierDecision {
        let lanes: Vec<(LaneRef, &Capabilities)> = self
            .members
            .iter()
            .map(|member| (member.lane, &member.caps))
            .collect();
        tier::decide(&lanes, declared, drift_failed)
    }

    pub(super) fn failure(&self, declared: Coherence) -> Option<ArrayFailure> {
        let down = self.lanes.iter().enumerate().find_map(|(slot, lane)| {
            let member = self.members.iter().find(|member| member.slot == slot);
            match (lane, member) {
                (None, _) | (Some(_), None) => Some(slot),
                (Some(_), Some(member)) => (!member.running).then_some(slot),
            }
        });
        if let Some(slot) = down {
            Some(ArrayFailure::DeviceDown { lane: slot as u32 })
        } else if self
            .members
            .iter()
            .any(|member| member.rate() != self.rate())
        {
            Some(ArrayFailure::RatesDiffer)
        } else if self.tier(declared, false).tier == Coherence::None {
            Some(ArrayFailure::NotCoherent)
        } else {
            None
        }
    }

    pub(super) fn caps(&self) -> BTreeMap<u32, Capabilities> {
        self.members
            .iter()
            .map(|member| (member.lane.device_set, member.caps.clone()))
            .collect()
    }

    fn settings(&self) -> BTreeMap<u32, DeviceSettings> {
        self.members
            .iter()
            .map(|member| (member.lane.device_set, member.settings.clone()))
            .collect()
    }

    pub(super) fn plan(
        &self,
        mode: ArrayTuningMode,
        tune: &ArrayTune,
    ) -> Result<tuner::TunePlan, EngineError> {
        let running: Vec<Option<LaneRef>> = self
            .lanes
            .iter()
            .map(|lane| {
                lane.filter(|lane| {
                    self.members
                        .iter()
                        .any(|member| member.lane == *lane && member.running)
                })
            })
            .collect();
        tuner::plan(&running, &self.caps(), &self.settings(), mode, tune)
            .map_err(EngineError::Array)
    }

    pub(super) fn gain_menu(&self) -> Option<tuner::GainMenu> {
        tuner::gain_menu(self.members.iter().map(|member| &member.caps))
    }

    pub(super) fn current_gain_db(&self) -> Option<f64> {
        let anchor = self.members.first()?;
        let lane = anchor
            .settings
            .for_stream(anchor.lane.stream, &anchor.caps.per_stream);
        tuner::main_stage(&anchor.caps).and_then(|stage| lane.gain(&stage.name))
    }

    pub(super) fn default_tune(&self) -> ArrayTune {
        let gain = self
            .current_gain_db()
            .filter(|db| (MIN_ARRAY_GAIN_DB..=MAX_ARRAY_GAIN_DB).contains(db))
            .map_or_else(ArrayGain::default, |db| ArrayGain::Manual { db });
        ArrayTune {
            center_hz: self
                .members
                .first()
                .map_or(DEFAULT_CENTER_HZ, Member::center),
            gain,
        }
    }

    pub(super) fn restarted(&self, leases: &[Option<Lease>]) -> Vec<usize> {
        self.members
            .iter()
            .filter(|member| {
                let held = leases.get(member.slot).and_then(Option::as_ref);
                let port = member.port.as_ref().filter(|_| self.usable(member));
                match (held, port) {
                    (Some(held), Some(port)) => !held.holds(port),
                    (None, None) => false,
                    _ => true,
                }
            })
            .map(|member| member.slot)
            .collect()
    }

    pub(super) fn lease(&self) -> Result<Leased, EngineError> {
        self.lease_where(|_| true)
    }

    pub(super) fn lease_where(
        &self,
        wanted: impl Fn(usize) -> bool,
    ) -> Result<Leased, EngineError> {
        let mut feeds: Vec<Option<LaneFeed>> = (0..self.lanes.len()).map(|_| None).collect();
        let mut leases: Vec<Option<Lease>> = (0..self.lanes.len()).map(|_| None).collect();
        let rate = self.rate();
        for member in self
            .members
            .iter()
            .filter(|member| wanted(member.slot) && self.usable(member))
        {
            let Some(port) = &member.port else { continue };
            match port.lease(rate) {
                Ok(feed) => {
                    leases[member.slot] = Some(Lease {
                        port: port.clone(),
                        id: feed.lease(),
                    });
                    feeds[member.slot] = Some(feed);
                }
                Err(error) => {
                    leases.iter().flatten().for_each(Lease::release);
                    return Err(error);
                }
            }
        }
        Ok((feeds, leases))
    }

    fn device_indexes(&self) -> [u8; MAX_LANES] {
        let mut devices = [u8::MAX; MAX_LANES];
        let mut seen: Vec<u32> = Vec::new();
        for member in &self.members {
            let index = seen
                .iter()
                .position(|ds| *ds == member.lane.device_set)
                .unwrap_or_else(|| {
                    seen.push(member.lane.device_set);
                    seen.len() - 1
                });
            if let Some(slot) = devices.get_mut(member.slot) {
                *slot = u8::try_from(index).unwrap_or(u8::MAX);
            }
        }
        devices
    }

    pub(super) fn frame(
        &self,
        spec: &ArraySpec,
        tier: &TierDecision,
        lane_centers_hz: &[f64],
        center_hz: f64,
    ) -> LiveFrame {
        LiveFrame {
            sample_rate: self.rate(),
            center_hz,
            lane_centers_hz: lane_centers_hz.to_vec(),
            orientation: spec.settings.orientation,
            tier: tier.tier,
            keeps_phase: tier.keeps_phase,
            needs_time: false,
            tuning: spec.settings.tuning,
            dc_block: self
                .members
                .iter()
                .any(|member| member.caps.dc_artifact == DcArtifact::Managed),
            in_flight: self.members.first().map_or(0, |member| member.in_flight),
            devices: self.device_indexes(),
        }
    }

    pub(super) fn shape(&self, spec: &ArraySpec) -> ArrayShape {
        ArrayShape {
            geometry: spec.settings.geometry.clone(),
            positions: self.positions.clone(),
            manifold: None,
            tuning: spec.settings.tuning,
        }
    }

    pub(super) fn noise(&self) -> Option<NoiseSwitch> {
        let source = self
            .members
            .iter()
            .find(|member| member.caps.noise_source != NoiseSource::None)?;
        let (held, reached) = match &source.caps.bank {
            Some(bank) => (self.on_bank(&bank.id), bank.lanes as usize),
            None => (
                self.on_device(source.lane.device_set),
                source.streams as usize,
            ),
        };
        Some(NoiseSwitch {
            device_set: source.lane.device_set,
            kind: source.caps.noise_source,
            all_lanes_held: held == reached,
        })
    }

    fn on_bank(&self, id: &str) -> usize {
        self.members
            .iter()
            .filter(|member| member.caps.bank.as_ref().is_some_and(|bank| bank.id == id))
            .count()
    }

    fn on_device(&self, device_set: u32) -> usize {
        self.members
            .iter()
            .filter(|member| member.lane.device_set == device_set)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_device::DeviceDriver;
    use sdrmm_device_virtual::VirtualDriver;
    use sdrmm_wire::{ClockBank, NoiseSource};

    use super::*;

    const BANK: &str = "001/4";
    const RATE: f64 = 2_400_000.0;

    fn dongle_caps() -> Capabilities {
        let driver = VirtualDriver::new();
        let info = driver
            .probe()
            .into_iter()
            .find(|info| info.key == "dongle1")
            .expect("the bench dongle");
        let device = driver.open(&info).expect("the bench dongle opens");
        device.capabilities().clone()
    }

    fn lane_caps(loose: &Capabilities, lane: u32, lanes: u32, source: NoiseSource) -> Capabilities {
        Capabilities {
            noise_source: source,
            bank: Some(ClockBank {
                id: BANK.to_owned(),
                lane,
                lanes,
            }),
            ..loose.clone()
        }
    }

    fn settled() -> DeviceSettings {
        DeviceSettings {
            center_hz: Some(433.92e6),
            sample_rate: Some(RATE),
            ..DeviceSettings::default()
        }
    }

    fn survey(held: u32, in_bank: u32) -> Survey {
        let loose = dongle_caps();
        let members: Vec<Member> = (0..held)
            .map(|lane| Member {
                slot: lane as usize,
                lane: LaneRef {
                    device_set: lane + 1,
                    stream: 0,
                },
                caps: lane_caps(
                    &loose,
                    lane,
                    in_bank,
                    if lane == 0 {
                        NoiseSource::Isolated
                    } else {
                        NoiseSource::None
                    },
                ),
                settings: settled(),
                running: true,
                port: None,
                in_flight: 0,
                streams: 1,
            })
            .collect();
        Survey {
            lanes: members.iter().map(|member| Some(member.lane)).collect(),
            members,
            positions: Vec::new(),
        }
    }

    #[test]
    fn dongles_on_one_bank_clock_make_a_coherent_array_without_being_declared() {
        let survey = survey(5, 5);
        let tier = survey.tier(Coherence::None, false);
        assert_eq!(tier.tier, Coherence::TimeSync);
        assert_eq!(tier.devices, 5);
        assert!(survey.failure(Coherence::None).is_none());
        let switch = survey.noise().expect("the bank's own noise source");
        assert_eq!(switch.device_set, 1);
        assert_eq!(switch.kind, NoiseSource::Isolated);
        assert!(
            switch.all_lanes_held,
            "the array holds every lane the source reaches"
        );
    }

    #[test]
    fn a_bank_lane_the_array_does_not_hold_leaves_the_noise_source_shared() {
        let switch = survey(4, 5).noise().expect("the bank's own noise source");
        assert!(
            !switch.all_lanes_held,
            "the fifth lane hears the source and is not in this array"
        );
    }

    #[test]
    fn a_lane_whose_radio_never_opened_takes_the_array_down() {
        let mut survey = survey(5, 5);
        survey.members.pop();
        assert_eq!(
            survey.failure(Coherence::None),
            Some(ArrayFailure::DeviceDown { lane: 4 })
        );
        survey.members.truncate(1);
        assert_eq!(
            survey.failure(Coherence::None),
            Some(ArrayFailure::DeviceDown { lane: 1 })
        );
    }
}
