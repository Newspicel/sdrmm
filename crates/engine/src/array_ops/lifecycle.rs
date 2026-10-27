use std::{collections::BTreeMap, sync::Arc};

use sdrmm_device::{DeviceError, LaneMark, lock};
use sdrmm_wire::{
    ArrayOrientation, ArrayTuningMode, LaneKey, NoiseSource, ServerEvent, StateScope,
};

use super::{ArrayState, Lease, find, find_mut, members};
use crate::{
    Engine, EngineError, Inner,
    array::{Command, ControlCommand, ControlConfig, LiveFrame, StatusBoard, SyncContext},
    lock_runtime,
    runtime::DeviceRuntime,
};

impl Engine {
    pub(crate) fn switch_array_noise(
        &self,
        node: &str,
        ds: u32,
        on: bool,
    ) -> Result<(), EngineError> {
        let (runtime, bank, board) = {
            let inner = self.lock();
            let array = match find(&inner, node) {
                Ok(array) => Some(array),
                Err(error) if on => return Err(error),
                Err(_) => None,
            };
            if on
                && let Some(array) = array
                && !array.uses(ds)
            {
                return Err(EngineError::Array(sdrmm_wire::ArrayFailure::NoNoiseSource));
            }
            let source = inner
                .device_sets
                .get(&ds)
                .ok_or(EngineError::DeviceSetNotFound(ds))?;
            if source.capabilities.noise_source == NoiseSource::None {
                return Err(EngineError::Array(sdrmm_wire::ArrayFailure::NoNoiseSource));
            }
            let bank = match (array, source.capabilities.bank.as_ref()) {
                (Some(array), Some(bank)) => bank_lanes(&inner, array, &bank.id),
                _ => BankLanes::default(),
            };
            (
                source.runtime.clone(),
                bank,
                array.map(|array| array.board.clone()),
            )
        };
        lock_runtime(&runtime).set_noise_source(on)?;
        if let Some(board) = board {
            mark_noise(&bank, on, &board);
        }
        Ok(())
    }

    pub(crate) fn arrays_lanes_lost(&self, ds: u32) {
        let mut inner = self.lock();
        let mut changed = false;
        for state in inner.arrays.values_mut().filter(|state| state.uses(ds)) {
            changed = true;
            let slots = state.slots_on(ds);
            let Some(first) = slots.first().copied() else {
                continue;
            };
            for slot in &slots {
                if let Some(lease) = state.leases.get_mut(*slot).and_then(Option::take) {
                    lease.release();
                }
            }
            state.failure = Some(sdrmm_wire::ArrayFailure::DeviceDown { lane: first as u32 });
            if let Err(error) = state.send(Command::LanesLost { slots }) {
                tracing::warn!(array = %state.spec.node, %error, "an array did not hear that its radio went down");
                state.board.add_events_lost(1);
            }
        }
        if changed {
            inner.revision += 1;
            drop(inner);
            self.emit(ServerEvent::StateChanged {
                scope: StateScope::Arrays,
            });
        }
    }

    pub(crate) fn arrays_after_reconnect(&self, ds: u32) {
        let arrays = self.arrays_on(ds);
        if arrays.is_empty() {
            return;
        }
        let _edits = lock(&self.array_edits);
        for node in arrays {
            if let Err(error) = self.rejoin(&node, ds) {
                tracing::warn!(array = %node, ds, %error, "an array did not take its radio back");
            }
        }
        self.emit(ServerEvent::StateChanged {
            scope: StateScope::Arrays,
        });
    }

    fn rejoin(&self, node: &str, ds: u32) -> Result<(), EngineError> {
        let delta = {
            let inner = self.lock();
            let state = find(&inner, node)?;
            members::survey(&inner, &state.spec)?
                .plan(state.spec.settings.tuning, &state.tune)?
                .deltas
                .into_iter()
                .find_map(|(planned, delta)| (planned == ds).then_some(delta))
        };
        if let Some(delta) = delta
            && let Err(error) = self.patch_device_quietly(ds, delta)
        {
            tracing::warn!(array = %node, ds, %error, "a returning radio did not take the array tune");
        }
        let anchor = {
            let mut inner = self.lock();
            let spec = find(&inner, node)?.spec.clone();
            let survey = members::survey(&inner, &spec)?;
            let state = find_mut(&mut inner, node)?;
            let rate = survey.rate();
            let mut slots = Vec::new();
            for member in survey
                .members
                .iter()
                .filter(|member| member.lane.device_set == ds && survey.usable(member))
            {
                let Some(port) = &member.port else { continue };
                let feed = port.lease(rate)?;
                let lease = Lease {
                    port: port.clone(),
                    id: feed.lease(),
                };
                if let Some(old) = state
                    .leases
                    .get_mut(member.slot)
                    .and_then(|held| held.replace(lease))
                {
                    old.release();
                }
                slots.push((member.slot, feed));
            }
            if !slots.is_empty() {
                state.send(Command::SwapFeeds { slots })?;
            }
            state.failure = survey.failure(spec.settings.declared);
            state.tell(ControlCommand::Resync { coarse: true })?;
            inner.revision += 1;
            find(&inner, node)?.anchor == Some(ds)
        };
        if anchor {
            self.reattach_lanes(node, ds);
        }
        Ok(())
    }

    pub(crate) fn arrays_rate_changed(&self, ds: u32) {
        let arrays = self.arrays_on(ds);
        if arrays.is_empty() {
            return;
        }
        let _edits = lock(&self.array_edits);
        for node in arrays {
            match self.rerate(&node) {
                Ok(true) => self.follow_rate(&node, ds),
                Ok(false) => {}
                Err(error) => {
                    tracing::warn!(array = %node, ds, %error, "an array did not follow a rate change");
                }
            }
        }
        self.emit(ServerEvent::StateChanged {
            scope: StateScope::Arrays,
        });
    }

    fn follow_rate(&self, node: &str, ds: u32) {
        let spread = self.lock().arrays.get(node).and_then(|state| {
            (state.spec.settings.tuning == ArrayTuningMode::Spread).then_some(state.tune)
        });
        if let Some(tune) = spread
            && let Err(error) = self.retune(node, tune, tune.gain)
        {
            tracing::warn!(array = %node, ds, %error, "a spread array did not follow a rate change");
        }
        self.reinstall_all(node, |_| true);
    }

    fn rerate(&self, node: &str) -> Result<bool, EngineError> {
        let mut inner = self.lock();
        let spec = find(&inner, node)?.spec.clone();
        let survey = members::survey(&inner, &spec)?;
        let state = find_mut(&mut inner, node)?;
        let rate = survey.rate();
        let rate_changed = rate != state.frame.sample_rate;
        let mut swaps = Vec::new();
        let mut lost = Vec::new();
        for member in &survey.members {
            if let Some(old) = state.leases.get_mut(member.slot).and_then(Option::take) {
                old.release();
            }
            let port = member.port.as_ref().filter(|_| survey.usable(member));
            match port {
                Some(port) => {
                    let feed = port.lease(rate)?;
                    if let Some(slot) = state.leases.get_mut(member.slot) {
                        *slot = Some(Lease {
                            port: port.clone(),
                            id: feed.lease(),
                        });
                    }
                    swaps.push((member.slot, feed));
                }
                None => lost.push(member.slot),
            }
        }
        let frame = LiveFrame {
            sample_rate: rate,
            ..state.frame.clone()
        };
        state.reframe(frame)?;
        if !swaps.is_empty() {
            state.send(Command::SwapFeeds { slots: swaps })?;
        }
        if !lost.is_empty() {
            state.send(Command::LanesLost { slots: lost })?;
        }
        state.failure = survey.failure(spec.settings.declared);
        let config = ControlConfig {
            sample_rate: rate,
            ..state.config.clone()
        };
        state.configure(config);
        state.tell(ControlCommand::Resync { coarse: true })?;
        inner.revision += 1;
        Ok(rate_changed)
    }

    pub(crate) fn arrays_capabilities_changed(&self, ds: u32) {
        let arrays = self.arrays_on(ds);
        if arrays.is_empty() {
            return;
        }
        let _edits = lock(&self.array_edits);
        for node in arrays {
            if let Err(error) = self.retier(&node) {
                tracing::warn!(array = %node, ds, %error, "an array did not follow a capability change");
            }
        }
        self.emit(ServerEvent::StateChanged {
            scope: StateScope::Arrays,
        });
    }

    pub(super) fn retier(&self, node: &str) -> Result<(), EngineError> {
        let mut inner = self.lock();
        let spec = find(&inner, node)?.spec.clone();
        let survey = members::survey(&inner, &spec)?;
        let state = find_mut(&mut inner, node)?;
        let tier = survey.tier(spec.settings.declared, state.drift.is_some());
        let mut frame = survey.frame(
            &spec,
            &tier,
            &state.frame.lane_centers_hz,
            state.tune.center_hz,
        );
        frame.needs_time = state.frame.needs_time;
        state.tier = tier;
        state.failure = survey.failure(spec.settings.declared);
        state.gain_menu = survey.gain_menu();
        let config = ControlConfig {
            tier,
            ..state.config.clone()
        };
        state.configure(config);
        state.reframe(frame)?;
        state.switch_noise(survey.noise());
        let restarted = survey.restarted(&state.leases);
        if !restarted.is_empty() {
            let leased = survey.lease_where(|slot| restarted.contains(&slot))?;
            state.swap_lanes(&restarted, leased)?;
        }
        inner.revision += 1;
        Ok(())
    }

    pub(crate) fn sync_context(&self, node: &str) -> Result<SyncContext, EngineError> {
        let inner = self.lock();
        let state = find(&inner, node)?;
        let lanes = state
            .spec
            .lanes
            .iter()
            .map(|lane| LaneKey {
                device: lane
                    .and_then(|lane| inner.device_sets.get(&lane.device_set))
                    .map(|device| device.info.id())
                    .unwrap_or_default(),
                stream: lane.map_or(0, |lane| lane.stream),
            })
            .collect();
        let azimuth_deg = match state.spec.settings.orientation {
            ArrayOrientation::Fixed { azimuth_deg } => Some(azimuth_deg),
            ArrayOrientation::Heading { mount_offset_deg } => state
                .pose
                .last()
                .and_then(|pose| pose.heading_deg)
                .map(|heading| (heading + mount_offset_deg).rem_euclid(360.0)),
        };
        Ok(SyncContext {
            lanes,
            center_hz: state.tune.center_hz,
            gain_db: state.board.control().gain_db,
            gain_steps_db: state
                .gain_menu
                .as_ref()
                .map_or_else(Vec::new, super::GainMenu::steps),
            positions: state.shape.positions.clone(),
            azimuth_deg,
            warm: state
                .spec
                .warm
                .clone()
                .filter(|_| state.spec.settings.cal.warm_start),
        })
    }

    pub(crate) fn clock_drift(&self, node: &str, ppm: Option<f64>) -> Result<(), EngineError> {
        let changed = {
            let mut inner = self.lock();
            let state = find_mut(&mut inner, node)?;
            let changed = state.drift.is_some() != ppm.is_some();
            state.drift = ppm;
            changed
        };
        if changed {
            self.retier(node)?;
            self.emit(ServerEvent::StateChanged {
                scope: StateScope::Arrays,
            });
        }
        Ok(())
    }
}

type BankLanes = Vec<(Arc<DeviceRuntime>, Vec<u32>)>;

fn bank_lanes(inner: &Inner, array: &ArrayState, id: &str) -> BankLanes {
    let mut held: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for lane in array.spec.lanes.iter().flatten() {
        let on_bank = inner.device_sets.get(&lane.device_set).is_some_and(|set| {
            set.capabilities
                .bank
                .as_ref()
                .is_some_and(|bank| bank.id == id)
        });
        if on_bank {
            held.entry(lane.device_set).or_default().push(lane.stream);
        }
    }
    held.into_iter()
        .filter_map(|(ds, streams)| {
            let set = inner.device_sets.get(&ds)?;
            Some((set.runtime.clone(), streams))
        })
        .collect()
}

fn mark_noise(bank: &BankLanes, on: bool, board: &StatusBoard) {
    for (runtime, streams) in bank {
        let (posters, in_flight) = {
            let runtime = lock_runtime(runtime);
            (runtime.mark_posters(), runtime.in_flight_samples())
        };
        let mark = LaneMark::NoiseSource { on, in_flight };
        for stream in streams {
            let posted = posters
                .get(*stream as usize)
                .ok_or_else(|| DeviceError::Io("lane has no mark queue".to_owned()))
                .and_then(|poster| poster.post(mark));
            if let Err(error) = posted {
                tracing::warn!(stream, %error, "a noise mark was lost");
                board.add_events_lost(1);
            }
        }
    }
}
