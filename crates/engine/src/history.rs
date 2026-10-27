use sdrmm_wire::{
    DeviceSetStatus, MAX_PRE_ROLL_BYTES, MAX_PRE_ROLL_S, MIN_PRE_ROLL_S, PreRollStatus,
    RecordingStatus, ServerEvent, StateScope, pre_roll_capacity_samples,
};

use crate::{
    DEFAULT_CENTER_HZ, DeviceSetState, Engine, EngineError, TIME_MACHINE_STOP_POLL,
    TIME_MACHINE_STOP_POLLS, recording,
    runtime::DspCommand,
    sample_rate_of, time_machine,
    time_machine::{TimeMachineCommand, TimeMachineControl, TimeMachineHandle, TimeMachineShared},
};

pub(crate) struct TimeMachineCapture {
    pub(crate) file: String,
    pub(crate) started_at: String,
    pub(crate) overruns_at_start: u64,
}

pub(crate) struct TimeMachineState {
    pub(crate) node: String,
    pub(crate) stream: u32,
    pub(crate) pre_roll_s: u32,
    pub(crate) sample_rate: u64,
    pub(crate) center_hz: i64,
    pub(crate) handle: TimeMachineHandle,
    pub(crate) control: TimeMachineControl,
    pub(crate) capture: Option<TimeMachineCapture>,
    pub(crate) overruns_at_start: u64,
    pub(crate) held_seen: u64,
    pub(crate) error_seen: bool,
}

impl TimeMachineState {
    pub(crate) fn status(&self, overruns_now: u64) -> PreRollStatus {
        let shared = self.handle.shared();
        PreRollStatus {
            node: self.node.clone(),
            stream: self.stream,
            pre_roll_s: self.pre_roll_s,
            sample_rate: self.sample_rate,
            center_hz: self.center_hz,
            held_samples: shared.held(),
            capacity_samples: self.handle.capacity(),
            overruns: overruns_now.saturating_sub(self.overruns_at_start),
            capture: self.capture.as_ref().map(|capture| RecordingStatus {
                file: capture.file.clone(),
                stream: self.stream,
                node: None,
                started_at: capture.started_at.clone(),
                samples: shared.captured(),
                bytes: shared.captured_bytes(),
                overruns: overruns_now.saturating_sub(capture.overruns_at_start),
                error: None,
            }),
            error: shared.error(),
        }
    }
}

fn await_finalized(shared: &TimeMachineShared, node: &str) {
    for _ in 0..TIME_MACHINE_STOP_POLLS {
        if !shared.capturing() {
            return;
        }
        std::thread::sleep(TIME_MACHINE_STOP_POLL);
    }
    tracing::warn!(
        node,
        "the time machine capture is still finalizing; its file lands when the keeper is done"
    );
}

fn named(node: &str) -> Result<(), EngineError> {
    if node.is_empty() || node.len() > sdrmm_wire::patch::MAX_NODE_ID_LEN {
        return Err(EngineError::Recording(
            "node id is empty or too long".to_owned(),
        ));
    }
    Ok(())
}

impl Engine {
    pub fn arm_pre_roll(
        &self,
        ds: u32,
        node: String,
        stream: u32,
        pre_roll_s: u32,
    ) -> Result<PreRollStatus, EngineError> {
        named(&node)?;
        let status = self.arm_history(ds, node, stream, pre_roll_s)?;
        self.emit(ServerEvent::StateChanged {
            scope: StateScope::DeviceSet(ds),
        });
        Ok(status)
    }

    pub fn drop_pre_roll(&self, ds: u32, node: &str) -> Result<PreRollStatus, EngineError> {
        named(node)?;
        let status = self.disarm_history(ds, node)?;
        self.emit(ServerEvent::StateChanged {
            scope: StateScope::DeviceSet(ds),
        });
        Ok(status)
    }

    pub fn start_pre_roll_capture(
        &self,
        ds: u32,
        node: &str,
    ) -> Result<PreRollStatus, EngineError> {
        named(node)?;
        let status = self.capture_time_machine(ds, node)?;
        self.emit(ServerEvent::StateChanged {
            scope: StateScope::DeviceSet(ds),
        });
        Ok(status)
    }

    pub fn stop_pre_roll_capture(&self, ds: u32, node: &str) -> Result<PreRollStatus, EngineError> {
        named(node)?;
        let status = self.stop_time_machine_capture(ds, node)?;
        self.emit(ServerEvent::StateChanged {
            scope: StateScope::DeviceSet(ds),
        });
        Ok(status)
    }

    fn arm_history(
        &self,
        ds: u32,
        node: String,
        stream: u32,
        pre_roll_s: u32,
    ) -> Result<PreRollStatus, EngineError> {
        if !(MIN_PRE_ROLL_S..=MAX_PRE_ROLL_S).contains(&pre_roll_s) {
            return Err(EngineError::Recording(format!(
                "a pre-roll of {pre_roll_s} s is outside {MIN_PRE_ROLL_S}..={MAX_PRE_ROLL_S} s"
            )));
        }
        let (rate, center) = {
            let inner = self.lock();
            let state = inner
                .device_sets
                .get(&ds)
                .ok_or(EngineError::DeviceSetNotFound(ds))?;
            state.check_physical(stream)?;
            if state.pre_roll.is_some() {
                return Err(EngineError::Recording(
                    "a recorder is already holding this radio's pre-roll".to_owned(),
                ));
            }
            if state.status != DeviceSetStatus::Running {
                return Err(EngineError::Recording(
                    "device set is not running".to_owned(),
                ));
            }
            let center = state
                .settings
                .for_stream(stream, &state.capabilities.per_stream)
                .center_hz
                .unwrap_or(DEFAULT_CENTER_HZ);
            (sample_rate_of(&state.settings), center)
        };
        let capacity = pre_roll_capacity_samples(pre_roll_s, rate);
        let bytes = capacity * sdrmm_recorder::BYTES_PER_SAMPLE;
        if bytes > MAX_PRE_ROLL_BYTES {
            let fits = MAX_PRE_ROLL_BYTES as f64 / (rate * sdrmm_recorder::BYTES_PER_SAMPLE as f64);
            return Err(EngineError::Recording(format!(
                "{pre_roll_s} s at {:.3} MS/s needs {} MiB of memory, above the {} MiB a pre-roll \
                 may take: this radio's rate leaves room for {:.0} s",
                rate / 1e6,
                bytes / (1 << 20),
                MAX_PRE_ROLL_BYTES / (1 << 20),
                fits.floor().max(0.0),
            )));
        }
        let mut handle = time_machine::start(capacity, rate, center)?;
        let Some(tap) = handle.take_tap() else {
            return Err(EngineError::Recording(
                "the history buffer started without a feed".to_owned(),
            ));
        };
        let control = handle.control();
        let committed = {
            let mut inner = self.lock();
            match inner.device_sets.get_mut(&ds) {
                Some(state)
                    if state.status == DeviceSetStatus::Running
                        && state.pre_roll.is_none()
                        && state.rate_patches == 0
                        && state.check_physical(stream).is_ok()
                        && sample_rate_of(&state.settings) == rate =>
                {
                    let history = TimeMachineState {
                        node,
                        stream,
                        pre_roll_s,
                        sample_rate: rate.round() as u64,
                        center_hz: center.round() as i64,
                        handle,
                        control,
                        capture: None,
                        overruns_at_start: state.overruns_total(),
                        held_seen: 0,
                        error_seen: false,
                    };
                    let status = history.status(state.overruns_total());
                    state.pre_roll = Some(history);
                    state.send_dsp(stream, DspCommand::StartTimeMachine { tap: Box::new(tap) });
                    inner.revision += 1;
                    Ok(status)
                }
                Some(state) if state.rate_patches > 0 => Err((
                    handle,
                    "a sample-rate change is in flight; retry once it completes",
                )),
                _ => Err((
                    handle,
                    "the radio stopped before its pre-roll could be held",
                )),
            }
        };
        committed.map_err(|(handle, reason)| {
            handle.join();
            EngineError::Recording(reason.to_owned())
        })
    }

    fn capture_time_machine(&self, ds: u32, node: &str) -> Result<PreRollStatus, EngineError> {
        let (control, shared, rate, center, hardware, stream) = {
            let inner = self.lock();
            let state = inner
                .device_sets
                .get(&ds)
                .ok_or(EngineError::DeviceSetNotFound(ds))?;
            let history = self.history_of(state, node)?;
            if history.capture.is_some() {
                return Err(EngineError::Recording(
                    "the time machine is already laying down a capture".to_owned(),
                ));
            }
            (
                history.control.clone(),
                history.handle.shared().clone(),
                history.sample_rate as f64,
                history.center_hz as f64,
                state.info.label.clone(),
                history.stream,
            )
        };
        let Some(dir) = self.recordings_dir.clone() else {
            return Err(EngineError::Recording(
                "no recordings directory configured".to_owned(),
            ));
        };
        std::fs::create_dir_all(&dir)
            .map_err(|e| EngineError::RecordingIo(format!("create {}: {e}", dir.display())))?;
        let started_at = jiff::Timestamp::now();
        let (writer, file) = recording::create_writer(
            &dir,
            &format!("tm_{ds}"),
            stream,
            started_at,
            rate,
            center,
            &hardware,
        )?;
        shared.expect_capture();
        control.send(TimeMachineCommand::Capture(Box::new(writer)))?;
        let status = {
            let mut inner = self.lock();
            let state = inner
                .device_sets
                .get_mut(&ds)
                .ok_or(EngineError::DeviceSetNotFound(ds))?;
            let overruns = state.overruns_total();
            let Some(history) = state.pre_roll.as_mut() else {
                return Err(EngineError::Recording(
                    "the time machine was disarmed while its capture started".to_owned(),
                ));
            };
            history.capture = Some(TimeMachineCapture {
                file,
                started_at: started_at.to_string(),
                overruns_at_start: overruns,
            });
            let status = history.status(overruns);
            inner.revision += 1;
            status
        };
        Ok(status)
    }

    fn stop_time_machine_capture(&self, ds: u32, node: &str) -> Result<PreRollStatus, EngineError> {
        let shared = {
            let inner = self.lock();
            let state = inner
                .device_sets
                .get(&ds)
                .ok_or(EngineError::DeviceSetNotFound(ds))?;
            let history = self.history_of(state, node)?;
            if history.capture.is_none() {
                return Err(EngineError::Recording(
                    "the time machine is not laying down a capture".to_owned(),
                ));
            }
            history.control.send(TimeMachineCommand::Stop)?;
            history.handle.shared().clone()
        };
        await_finalized(&shared, node);
        let status = {
            let mut inner = self.lock();
            let state = inner
                .device_sets
                .get_mut(&ds)
                .ok_or(EngineError::DeviceSetNotFound(ds))?;
            let overruns = state.overruns_total();
            let Some(history) = state.pre_roll.as_mut() else {
                return Err(EngineError::Recording(
                    "the time machine was disarmed while its capture stopped".to_owned(),
                ));
            };
            let status = history.status(overruns);
            history.capture = None;
            inner.revision += 1;
            status
        };
        self.emit(ServerEvent::StateChanged {
            scope: StateScope::Recordings,
        });
        Ok(status)
    }

    fn disarm_history(&self, ds: u32, node: &str) -> Result<PreRollStatus, EngineError> {
        let (history, overruns) = {
            let mut inner = self.lock();
            let state = inner
                .device_sets
                .get_mut(&ds)
                .ok_or(EngineError::DeviceSetNotFound(ds))?;
            let owner = state.pre_roll.as_ref().map(|history| history.node.clone());
            match owner {
                Some(owner) if owner == node => {}
                Some(owner) => {
                    return Err(EngineError::Recording(format!(
                        "the time machine belongs to node `{owner}`"
                    )));
                }
                None => {
                    return Err(EngineError::Recording(
                        "no time machine is holding this radio's history".to_owned(),
                    ));
                }
            }
            let Some(history) = state.pre_roll.take() else {
                return Err(EngineError::Recording(
                    "the time machine vanished while disarming".to_owned(),
                ));
            };
            state.send_dsp(history.stream, DspCommand::StopTimeMachine);
            let overruns = state.overruns_total();
            inner.revision += 1;
            (history, overruns)
        };
        let captured = history.capture.is_some();
        let status = history.status(overruns);
        history.handle.join();
        if captured {
            self.emit(ServerEvent::StateChanged {
                scope: StateScope::Recordings,
            });
        }
        Ok(status)
    }

    fn history_of<'a>(
        &self,
        state: &'a DeviceSetState,
        node: &str,
    ) -> Result<&'a TimeMachineState, EngineError> {
        let history = state.pre_roll.as_ref().ok_or_else(|| {
            EngineError::Recording("no time machine is holding this radio's history".to_owned())
        })?;
        if history.node == node {
            Ok(history)
        } else {
            Err(EngineError::Recording(format!(
                "the time machine belongs to node `{}`",
                history.node
            )))
        }
    }
}
