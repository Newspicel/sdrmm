use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::JoinHandle,
};

use sdrmm_channels::{ChannelCtx, TxPayload};
use sdrmm_wire::{
    ChannelParams, ChannelSettings, DeviceSetStatus, Direction, ServerEvent, StateScope,
    TransmitPhase, TransmitSettings, TransmitStatus,
};

use crate::{
    DeviceSetState, Engine, EngineError, Inner, center_of, runtime::DspCommand, sample_rate_of,
};

mod worker;

pub(crate) use worker::TxFeedTap;
use worker::{Band, Source, TxCommand, TxShared, Worker};

/// What a transmitter sends.
#[derive(Clone, Debug, PartialEq)]
pub enum TransmitSource {
    /// A decoder's own modulator, fed audio or messages.
    Modulator(ChannelParams),
    /// A lane as it is received: a recording, a generator or another radio.
    Lane { device_set: u32, stream: u32 },
    /// One channel's filtered baseband, as its decoder sees it.
    Channel { device_set: u32, channel: u32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransmitRequest {
    pub node: String,
    pub source: TransmitSource,
    pub settings: TransmitSettings,
}

/// What changed since the state was last published.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Seen {
    phase: TransmitPhase,
    bursts: u64,
    failed: bool,
}

pub(crate) struct TransmitState {
    request: TransmitRequest,
    modulates: Option<Modulates>,
    feed: Option<FeedFrom>,
    commands: mpsc::Sender<TxCommand>,
    shared: Arc<TxShared>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    seen: Seen,
}

struct Modulates {
    type_id: String,
    audio: bool,
}

impl TransmitState {
    pub(crate) fn status(&self) -> TransmitStatus {
        self.shared
            .status(&self.request.node, &self.request.settings)
    }

    /// True when the transmitter keyed, unkeyed or failed since the last call.
    pub(crate) fn changed(&mut self) -> bool {
        let status = self.status();
        let now = Seen {
            phase: status.phase,
            bursts: status.bursts,
            failed: status.error.is_some(),
        };
        std::mem::replace(&mut self.seen, now) != now
    }

    pub(crate) fn halt(mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take()
            && worker.join().is_err()
        {
            tracing::error!(node = %self.request.node, "the transmit thread panicked");
        }
    }

    fn send(&self, command: TxCommand) -> Result<(), EngineError> {
        if self.shared.stopped() || self.commands.send(command).is_err() {
            let reason = self
                .status()
                .error
                .unwrap_or_else(|| "it has stopped".to_owned());
            return Err(EngineError::Transmit(format!("disarmed: {reason}")));
        }
        Ok(())
    }
}

/// Where a feed is tapped: a lane, or one channel on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FeedFrom {
    device_set: u32,
    stream: u32,
    channel: Option<u32>,
}

impl FeedFrom {
    fn band(self, state: &DeviceSetState) -> Option<Band> {
        match self.channel {
            None => Some(radio_band(state, self.stream)),
            Some(channel) => state
                .baseband_plan(self.device_set, channel)
                .ok()
                .map(|plan| Band {
                    center_hz: plan.center_hz,
                    sample_rate: plan.sample_rate,
                }),
        }
    }

    fn start(self, tap: TxFeedTap) -> DspCommand {
        match self.channel {
            None => DspCommand::StartTransmitFeed { tap },
            Some(id) => DspCommand::StartChannelTransmitFeed { id, tap },
        }
    }

    const fn stop(self) -> DspCommand {
        match self.channel {
            None => DspCommand::StopTransmitFeed,
            Some(id) => DspCommand::StopChannelTransmitFeed { id },
        }
    }
}

struct Plan {
    radio: Band,
    source: Planned,
    runtime: Arc<crate::runtime::DeviceRuntime>,
}

enum Planned {
    Modulator {
        params: ChannelParams,
        rate: f64,
        audio: bool,
        width_hz: f64,
    },
    Feed {
        from: FeedFrom,
        band: Band,
    },
}

fn refused(reason: impl Into<String>) -> EngineError {
    EngineError::Transmit(reason.into())
}

fn radio_band(state: &DeviceSetState, stream: u32) -> Band {
    Band {
        center_hz: center_of(&state.settings, stream, &state.capabilities.per_stream),
        sample_rate: sample_rate_of(&state.settings),
    }
}

impl Inner {
    fn running(&self, ds: u32) -> Result<&DeviceSetState, EngineError> {
        let state = self
            .device_sets
            .get(&ds)
            .ok_or(EngineError::DeviceSetNotFound(ds))?;
        if state.status != DeviceSetStatus::Running {
            return Err(refused("the radio is not running"));
        }
        Ok(state)
    }

    fn transmitter(&self, ds: u32, node: &str) -> Result<&TransmitState, EngineError> {
        self.device_sets
            .get(&ds)
            .ok_or(EngineError::DeviceSetNotFound(ds))?
            .transmit
            .as_ref()
            .filter(|transmit| transmit.request.node == node)
            .ok_or_else(|| refused("the transmitter is not armed"))
    }

    fn plan(&self, ds: u32, request: &TransmitRequest) -> Result<Plan, EngineError> {
        let state = self.running(ds)?;
        if !state.capabilities.duplex.supports(Direction::Tx) {
            return Err(refused("this radio does not transmit"));
        }
        if let Some(held) = state
            .transmit
            .as_ref()
            .filter(|held| held.request.node != request.node)
        {
            return Err(refused(format!(
                "this radio already carries transmitter {}",
                held.request.node
            )));
        }
        let source = match &request.source {
            TransmitSource::Modulator(params) => {
                let descriptor = sdrmm_channels::descriptor(params.type_id())
                    .ok_or_else(|| refused(format!("no decoder named {}", params.type_id())))?;
                if !descriptor.can_transmit {
                    return Err(refused(format!("{} has no modulator", descriptor.name)));
                }
                let (low, high) = sdrmm_channels::layout(params)?.band;
                Planned::Modulator {
                    params: params.clone(),
                    rate: descriptor.input_rate_hz,
                    audio: descriptor.transmits_audio,
                    width_hz: high - low,
                }
            }
            TransmitSource::Lane { device_set, stream } => {
                let lane = self.running(*device_set)?;
                lane.dsp_sender(*stream)?;
                Planned::Feed {
                    from: FeedFrom {
                        device_set: *device_set,
                        stream: *stream,
                        channel: None,
                    },
                    band: radio_band(lane, *stream),
                }
            }
            TransmitSource::Channel {
                device_set,
                channel,
            } => {
                let lane = self.running(*device_set)?;
                let tapped = lane.baseband_plan(*device_set, *channel)?;
                Planned::Feed {
                    from: FeedFrom {
                        device_set: *device_set,
                        stream: tapped.stream,
                        channel: Some(*channel),
                    },
                    band: Band {
                        center_hz: tapped.center_hz,
                        sample_rate: tapped.sample_rate,
                    },
                }
            }
        };
        Ok(Plan {
            radio: radio_band(state, 0),
            source,
            runtime: state.runtime.clone(),
        })
    }

    /// Tells every transmitter that sends from or through `ds` where that radio now sits.
    pub(crate) fn transmitters_follow(&self, ds: u32) {
        let Some(moved) = self.device_sets.get(&ds) else {
            return;
        };
        for (id, state) in &self.device_sets {
            let Some(transmit) = &state.transmit else {
                continue;
            };
            let told = if *id == ds {
                transmit.send(TxCommand::Radio(radio_band(moved, 0)))
            } else {
                Ok(())
            };
            let fed = match transmit.feed {
                Some(from) if from.device_set == ds => from
                    .band(moved)
                    .map_or(Ok(()), |band| transmit.send(TxCommand::Feed(band))),
                _ => Ok(()),
            };
            let told = told.and(fed);
            if let Err(error) = told {
                tracing::debug!(ds = *id, %error, "a stopped transmitter was not retuned");
            }
        }
    }
}

struct Started {
    state: TransmitState,
    tap: Option<(FeedFrom, TxFeedTap)>,
}

fn start(plan: Plan, request: &TransmitRequest) -> Result<Started, EngineError> {
    let shared = Arc::new(TxShared::default());
    let (source, modulates, tap) = match plan.source {
        Planned::Modulator {
            params,
            rate,
            audio,
            width_hz,
        } => {
            shared.declare_channel(width_hz);
            let type_id = params.type_id().to_owned();
            let settings = ChannelSettings::new(plan.radio.center_hz, params);
            let tx = sdrmm_channels::create_tx(ChannelCtx { input_rate: rate }, &settings)?;
            let source = Source::Modulator { tx, rate };
            (source, Some(Modulates { type_id, audio }), None)
        }
        Planned::Feed { from, band } => {
            let (tap, source) = worker::feed(band, &shared);
            (source, None, Some((from, tap)))
        }
    };
    let stream = plan.runtime.lock().tx_start()?;
    let (commands, received) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let worker = Worker::new(
        stream,
        source,
        plan.radio,
        request.settings.clone(),
        shared.clone(),
    );
    let halt = stop.clone();
    let handle = std::thread::Builder::new()
        .name("sdrmm-transmit".to_owned())
        .spawn(move || {
            sdrmm_device::schedule::claim(sdrmm_device::Latency::Critical);
            worker.run(&received, &halt);
        })
        .map_err(|error| refused(format!("spawn transmit thread: {error}")))?;
    let feed = tap.as_ref().map(|(from, _)| *from);
    let state = TransmitState {
        request: request.clone(),
        modulates,
        feed,
        commands,
        shared,
        stop,
        worker: Some(handle),
        seen: Seen {
            phase: TransmitPhase::Idle,
            bursts: 0,
            failed: false,
        },
    };
    Ok(Started { state, tap })
}

impl Engine {
    /// Arms the radio's transmitter: from here on, whatever the source produces goes on the air.
    pub fn arm_transmitter(
        &self,
        ds: u32,
        request: &TransmitRequest,
    ) -> Result<TransmitStatus, EngineError> {
        if let Some(problem) = request.settings.problem() {
            return Err(refused(problem));
        }
        let plan = self.lock().plan(ds, request)?;
        self.halt_transmitter(ds);
        let Started { state, tap } = start(plan, request)?;
        let mut inner = self.lock();
        let free = inner
            .running(ds)
            .is_ok_and(|radio| radio.transmit.is_none());
        let fed = tap.as_ref().is_none_or(|(from, _)| {
            inner
                .running(from.device_set)
                .is_ok_and(|lane| from.band(lane).is_some() && lane.dsp_sender(from.stream).is_ok())
        });
        if !(free && fed) {
            drop(inner);
            state.halt();
            return Err(refused("the radio changed while the transmitter armed"));
        }
        if let Some((from, tap)) = tap
            && let Some(lane) = inner.device_sets.get(&from.device_set)
        {
            lane.send_dsp(from.stream, from.start(tap));
        }
        let status = state.status();
        if let Some(radio) = inner.device_sets.get_mut(&ds) {
            radio.transmit = Some(state);
        }
        inner.revision += 1;
        drop(inner);
        self.emit(ServerEvent::StateChanged {
            scope: StateScope::DeviceSet(ds),
        });
        Ok(status)
    }

    pub fn disarm_transmitter(&self, ds: u32, node: &str) -> Result<(), EngineError> {
        self.lock().transmitter(ds, node)?;
        self.halt_transmitter(ds);
        Ok(())
    }

    /// Takes whatever transmitter the radio carries off the air and waits for it to end.
    pub(crate) fn halt_transmitter(&self, ds: u32) {
        let taken = {
            let mut inner = self.lock();
            let taken = inner
                .device_sets
                .get_mut(&ds)
                .and_then(|state| state.transmit.take());
            if let Some(from) = taken.as_ref().and_then(|transmit| transmit.feed)
                && let Some(lane) = inner.device_sets.get(&from.device_set)
            {
                lane.send_dsp(from.stream, from.stop());
            }
            if taken.is_some() {
                inner.revision += 1;
            }
            taken
        };
        if let Some(transmit) = taken {
            transmit.halt();
            self.emit(ServerEvent::StateChanged {
                scope: StateScope::DeviceSet(ds),
            });
        }
    }

    pub fn patch_transmitter(
        &self,
        ds: u32,
        node: &str,
        settings: TransmitSettings,
    ) -> Result<TransmitStatus, EngineError> {
        if let Some(problem) = settings.problem() {
            return Err(refused(problem));
        }
        let mut inner = self.lock();
        inner.transmitter(ds, node)?;
        let Some(transmit) = inner
            .device_sets
            .get_mut(&ds)
            .and_then(|state| state.transmit.as_mut())
        else {
            return Err(refused("the transmitter is not armed"));
        };
        transmit.send(TxCommand::Settings(settings.clone()))?;
        transmit.request.settings = settings;
        let status = transmit.status();
        inner.revision += 1;
        drop(inner);
        self.emit(ServerEvent::StateChanged {
            scope: StateScope::DeviceSet(ds),
        });
        Ok(status)
    }

    /// Every armed transmitter with the radio it drives, as it was asked for.
    #[must_use]
    pub fn transmitters(&self) -> Vec<(u32, TransmitRequest)> {
        self.lock()
            .device_sets
            .iter()
            .filter_map(|(ds, state)| Some((*ds, state.transmit.as_ref()?.request.clone())))
            .collect()
    }

    /// Sends one message, shaped by the form the modulator's decoder declares.
    pub fn transmit_message(
        &self,
        ds: u32,
        node: &str,
        message: &serde_json::Value,
    ) -> Result<(), EngineError> {
        let inner = self.lock();
        let transmit = inner.transmitter(ds, node)?;
        let Some(modulates) = &transmit.modulates else {
            return Err(refused("a lane takes no messages"));
        };
        let payload = sdrmm_channels::compose(&modulates.type_id, message)?;
        transmit.send(TxCommand::Payload(payload))
    }

    /// Feeds mono audio at the modulator's rate to a voice transmitter.
    pub fn transmit_audio(&self, ds: u32, node: &str, pcm: Vec<f32>) -> Result<(), EngineError> {
        let inner = self.lock();
        let transmit = inner.transmitter(ds, node)?;
        if !transmit.modulates.as_ref().is_some_and(|m| m.audio) {
            return Err(refused("this transmitter takes no audio"));
        }
        transmit.send(TxCommand::Payload(TxPayload::Audio(pcm)))
    }
}
