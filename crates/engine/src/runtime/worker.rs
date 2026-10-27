use std::{
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use arc_swap::ArcSwap;

use super::{
    ChannelHost, DspCommand, DspMeta, MAX_LANE_CHANNELS, MAX_LANE_MONITORS, dsp_block_len,
    frontend::Frontend,
    retire::Reclaimer,
    spectrum::{average::FrameAverage, history::SpectrumHistory},
    subbands::Subbands,
};
use crate::{
    audio_recording::AudioTaps,
    capture_ring::CaptureConsumer,
    network_export::NetworkExportTap,
    publishing::{recording::RecordingPublisher, spectrum::SpectrumPublisher},
    recording::RecorderTap,
    spectrum::{SpectrumAnalyzer, SpectrumFrame},
    time_machine::TimeMachineTap,
};

const TARGET_FPS: f64 = 30.0;
const SPECTRUM_AVERAGES: u32 = 16;
const IDLE_PARK: Duration = Duration::from_millis(20);

#[derive(Default)]
pub(crate) struct Waker(OnceLock<std::thread::Thread>);

impl Waker {
    pub(crate) fn adopt_current(&self) {
        let _ = self.0.set(std::thread::current());
    }

    pub(crate) fn wake(&self) {
        if let Some(thread) = self.0.get() {
            thread.unpark();
        }
    }
}

pub(super) struct LaneShared {
    pub(super) meta: Arc<ArcSwap<DspMeta>>,
    pub(super) stop: Arc<AtomicBool>,
    pub(super) stalled_us: Arc<AtomicU64>,
    pub(super) waker: Arc<Waker>,
    pub(super) max_age: Duration,
    pub(super) spectrum_size: usize,
}

pub(super) enum Retired {
    Subbands(Box<Subbands>),
    Monitor(Box<crate::monitor::MonitorTap>),
    Channel(Box<ChannelHost>),
    Recording(Option<RecorderTap>, Option<RecordingPublisher>),
    Network(NetworkExportTap),
    History(TimeMachineTap),
    AudioTaps(Option<AudioTaps>),
}

impl Retired {
    pub(super) fn release(self) {
        match self {
            Self::Subbands(bands) => drop(bands),
            Self::Monitor(tap) => drop(tap),
            Self::Channel(host) => drop(host),
            Self::Recording(tap, publisher) => release_recording(tap, publisher),
            Self::Network(tap) => drop(tap),
            Self::History(tap) => drop(tap),
            Self::AudioTaps(taps) => drop(taps),
        }
    }
}

fn release_recording(tap: Option<RecorderTap>, publisher: Option<RecordingPublisher>) {
    if publisher.is_none() {
        drop(tap);
        return;
    }
    let handed = std::thread::Builder::new()
        .name("sdrmm-iq-retire".to_owned())
        .spawn(move || drop((publisher, tap)));
    if let Err(error) = handed {
        tracing::error!(%error, "recording teardown had no thread and closed here");
    }
}

fn hosted_channels() -> Vec<(u32, Box<ChannelHost>)> {
    Vec::with_capacity(MAX_LANE_CHANNELS)
}

fn hosted_monitors() -> Vec<(u64, Box<crate::monitor::MonitorTap>)> {
    Vec::with_capacity(MAX_LANE_MONITORS)
}

pub(super) fn dsp_loop(
    consumer: &mut CaptureConsumer,
    commands: &mpsc::Receiver<DspCommand>,
    lane: &LaneShared,
    mut analyzer: SpectrumAnalyzer,
    mut publisher: SpectrumPublisher,
    mut retirement: Reclaimer<Retired>,
) {
    let LaneShared {
        meta,
        stop,
        stalled_us,
        max_age,
        spectrum_size,
        ..
    } = lane;
    let size = *spectrum_size;
    let mut spectrum_history = SpectrumHistory::new(size);
    let mut db = vec![0.0f32; size];
    let mut frame_average = FrameAverage::new(size);
    let mut averaged = vec![0.0f32; size];
    let mut channels = hosted_channels();
    let mut monitors = hosted_monitors();
    let mut tap: Option<RecorderTap> = None;
    let mut recording_publisher: Option<RecordingPublisher> = None;
    let mut network_tap: Option<NetworkExportTap> = None;
    let mut history: Option<TimeMachineTap> = None;
    let mut seq: u32 = 0;
    let mut next_input = None;
    let mut served = Instant::now();
    let mut frontend = Frontend::new(*meta.load_full());
    let mut subbands = Box::new(Subbands::new(meta.load().sample_rate));

    while !stop.load(Ordering::Acquire) {
        drain_commands(
            commands,
            &mut channels,
            &mut subbands,
            CommandSinks {
                monitors: &mut monitors,
                tap: &mut tap,
                network_tap: &mut network_tap,
                history: &mut history,
                recording_publisher: &mut recording_publisher,
            },
            &mut retirement,
        );
        let snapshot = *meta.load_full();
        let (hop, averages) = spectrum_cadence(snapshot.sample_rate, size);
        frontend.follow(snapshot);
        subbands.prepare(&mut channels, snapshot.center_hz, snapshot.sample_rate);
        let block_len = dsp_block_len(snapshot.sample_rate);
        let consumed = consumer.consume_fresh(block_len, *max_age, |raw, total| {
            record_stall(stalled_us, &mut served);
            if next_input.is_some_and(|next| next != total) {
                frontend.reset();
                spectrum_history.reset();
                if let Some(partial) = frame_average.flush(&mut averaged) {
                    seq = seq.wrapping_add(1);
                    publisher.publish(seq, partial, &averaged);
                }
            }
            next_input = Some(total + raw.len() as u64);
            let slice = frontend.apply(raw);
            subbands.process(slice, total);
            for (_, monitor) in &mut monitors {
                monitor.push(slice, total, snapshot);
            }
            if tap.as_ref().is_some_and(|t| {
                recording_publisher
                    .as_mut()
                    .is_none_or(|publisher| !publisher.publish(t, slice, total, snapshot.center_hz))
            }) && let Some(slot) = retirement.slot()
            {
                slot.retire(Retired::Recording(tap.take(), recording_publisher.take()));
            }
            if network_tap
                .as_mut()
                .is_some_and(|network| !network.push(slice))
                && let Some(slot) = retirement.slot()
                && let Some(network) = network_tap.take()
            {
                slot.retire(Retired::Network(network));
            }
            if history
                .as_mut()
                .is_some_and(|keeper| !keeper.push(slice, snapshot.center_hz))
                && let Some(slot) = retirement.slot()
                && let Some(keeper) = history.take()
            {
                slot.retire(Retired::History(keeper));
            }
            for (_, host) in &mut channels {
                let selected = host
                    .subband(snapshot.center_hz, snapshot.sample_rate)
                    .and_then(|band| subbands.samples(band));
                if selected.is_some() {
                    host.process_shared_at(slice, total, snapshot.center_hz, selected);
                } else {
                    host.process_at(slice, total, snapshot.center_hz);
                }
            }
            spectrum_history.push(slice, total, hop, |window, timestamp| {
                let frame = SpectrumFrame {
                    timestamp,
                    center_hz: snapshot.center_hz,
                    span_hz: snapshot.sample_rate as f32,
                };
                if let Some(completed) = analyzer.power_db(window, &mut db, frame)
                    && let Some(done) = frame_average.push(completed, &db, averages, &mut averaged)
                {
                    seq = seq.wrapping_add(1);
                    publisher.publish(seq, done, &averaged);
                }
            });
        });
        if consumed == 0 {
            if spectrum_history.settling(hop)
                && let Some(partial) = frame_average.flush(&mut averaged)
            {
                seq = seq.wrapping_add(1);
                publisher.publish(seq, partial, &averaged);
            }
            std::thread::park_timeout(IDLE_PARK);
        }
    }
    release_recording(tap.take(), recording_publisher.take());
}

fn spectrum_cadence(sample_rate: f64, size: usize) -> (usize, u32) {
    let per_frame = (sample_rate / TARGET_FPS).max(1.0);
    let hop = ((per_frame / f64::from(SPECTRUM_AVERAGES)) as usize).max(size / 4);
    let averages = (per_frame / hop as f64).round() as u32;
    (hop, averages.clamp(1, SPECTRUM_AVERAGES))
}

fn record_stall(stalled_us: &AtomicU64, served: &mut Instant) {
    let now = Instant::now();
    let gap = u64::try_from(now.duration_since(*served).as_micros()).unwrap_or(u64::MAX);
    *served = now;
    stalled_us.fetch_max(gap, Ordering::Relaxed);
}

struct CommandSinks<'a> {
    monitors: &'a mut Vec<(u64, Box<crate::monitor::MonitorTap>)>,
    tap: &'a mut Option<RecorderTap>,
    network_tap: &'a mut Option<NetworkExportTap>,
    history: &'a mut Option<TimeMachineTap>,
    recording_publisher: &'a mut Option<RecordingPublisher>,
}

fn drain_commands(
    commands: &mpsc::Receiver<DspCommand>,
    channels: &mut Vec<(u32, Box<ChannelHost>)>,
    subbands: &mut Box<Subbands>,
    sinks: CommandSinks<'_>,
    retirement: &mut Reclaimer<Retired>,
) {
    let CommandSinks {
        monitors,
        tap,
        network_tap,
        history,
        recording_publisher,
    } = sinks;
    for _ in 0..64 {
        let Some(slot) = retirement.slot() else {
            break;
        };
        let Ok(cmd) = commands.try_recv() else {
            break;
        };
        match cmd {
            DspCommand::AddMonitor { id, tap } => {
                if monitors.len() < MAX_LANE_MONITORS {
                    monitors.push((id, tap));
                } else {
                    tracing::error!(
                        id,
                        held = MAX_LANE_MONITORS,
                        "this lane holds no more monitors"
                    );
                    slot.retire(Retired::Monitor(tap));
                }
            }
            DspCommand::RemoveMonitor { id } => {
                if let Some(index) = monitors.iter().position(|(held, _)| *held == id) {
                    let (_, tap) = monitors.remove(index);
                    slot.retire(Retired::Monitor(tap));
                }
            }
            DspCommand::SetSubbands(bands) => {
                slot.retire(Retired::Subbands(std::mem::replace(subbands, bands)));
            }
            DspCommand::AddChannel {
                id,
                mut host,
                reset_state,
            } => {
                if let Some((_, previous)) =
                    channels.iter_mut().find(|(existing, _)| *existing == id)
                {
                    if reset_state {
                        host.follow(previous);
                    } else {
                        host.inherit(previous);
                    }
                    std::mem::swap(previous, &mut host);
                    slot.retire(Retired::Channel(host));
                } else if channels.len() < MAX_LANE_CHANNELS {
                    channels.push((id, host));
                } else {
                    tracing::error!(
                        id,
                        held = MAX_LANE_CHANNELS,
                        "this lane holds no more channels"
                    );
                    slot.retire(Retired::Channel(host));
                }
            }
            DspCommand::RemoveChannel { id } => {
                if let Some(index) = channels.iter().position(|(existing, _)| *existing == id) {
                    slot.retire(Retired::Channel(channels.remove(index).1));
                }
            }
            DspCommand::RetuneChannel { id, frequency_hz } => {
                if let Some((_, host)) = channels.iter_mut().find(|(existing, _)| *existing == id) {
                    host.retune(frequency_hz);
                } else {
                    tracing::debug!(id, "retune for a channel no longer hosted");
                }
            }
            DspCommand::SteerChannel { id, doppler } => {
                if let Some((_, host)) = channels.iter_mut().find(|(existing, _)| *existing == id) {
                    host.steer(doppler);
                } else {
                    tracing::debug!(id, "doppler for a channel no longer hosted");
                }
            }
            DspCommand::PositionChanged { id, fix } => {
                if let Some((_, host)) = channels.iter_mut().find(|(existing, _)| *existing == id) {
                    host.position_changed(fix.as_ref());
                } else {
                    tracing::debug!(id, "position for a channel no longer hosted");
                }
            }
            DspCommand::StartRecording {
                tap: armed,
                publisher,
            } => {
                slot.retire(Retired::Recording(
                    tap.replace(armed),
                    recording_publisher.replace(publisher),
                ));
            }
            DspCommand::StopRecording => {
                slot.retire(Retired::Recording(tap.take(), recording_publisher.take()));
            }
            DspCommand::SetChannelRecordings { id, taps } => {
                match channels.iter_mut().find(|(existing, _)| *existing == id) {
                    Some((_, host)) => {
                        slot.retire(Retired::AudioTaps(host.set_audio_recordings(taps)));
                    }
                    None => {
                        tracing::debug!(id, "audio recording for a channel no longer hosted");
                        slot.retire(Retired::AudioTaps(taps));
                    }
                }
            }
            DspCommand::StartBasebandRecording { id, tap: armed } => {
                match channels.iter_mut().find(|(existing, _)| *existing == id) {
                    Some((_, host)) => host.set_baseband_recording(Some(armed)),
                    None => {
                        tracing::debug!(id, "baseband recording for a channel no longer hosted");
                    }
                }
            }
            DspCommand::StopBasebandRecording { id } => {
                if let Some((_, host)) = channels.iter_mut().find(|(existing, _)| *existing == id) {
                    host.set_baseband_recording(None);
                }
            }
            DspCommand::StartBasebandExport { id, tap: armed } => {
                match channels.iter_mut().find(|(existing, _)| *existing == id) {
                    Some((_, host)) => host.set_baseband_export(Some(armed)),
                    None => tracing::debug!(id, "baseband export for a channel no longer hosted"),
                }
            }
            DspCommand::StopBasebandExport { id } => {
                if let Some((_, host)) = channels.iter_mut().find(|(existing, _)| *existing == id) {
                    host.set_baseband_export(None);
                }
            }
            DspCommand::StartNetworkExport { tap: armed } => {
                if let Some(old) = network_tap.replace(armed) {
                    slot.retire(Retired::Network(old));
                }
            }
            DspCommand::StopNetworkExport => {
                if let Some(old) = network_tap.take() {
                    slot.retire(Retired::Network(old));
                }
            }
            DspCommand::StartTimeMachine { tap: armed } => {
                if let Some(old) = history.replace(*armed) {
                    slot.retire(Retired::History(old));
                }
            }
            DspCommand::StopTimeMachine => {
                if let Some(old) = history.take() {
                    slot.retire(Retired::History(old));
                }
            }
            #[cfg(test)]
            DspCommand::Hold(hold) => hold(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_published_frame_averages_the_transforms_that_fit_its_period() {
        for rate in [2_400_000.0, 20_000_000.0, 61_440_000.0, 245_760_000.0] {
            let (hop, averages) = spectrum_cadence(rate, crate::runtime::spectrum_size(rate));
            assert_eq!(averages, SPECTRUM_AVERAGES);
            let covered = hop as f64 * f64::from(averages) / rate;
            assert!(
                (covered - 1.0 / TARGET_FPS).abs() < 1e-3,
                "{rate}: {covered} s"
            );
        }
        assert_eq!(spectrum_cadence(48_000.0, 4_096), (1_024, 2));
        assert_eq!(spectrum_cadence(8_000.0, 4_096).1, 1);
    }

    #[test]
    fn taking_in_a_channel_allocates_nothing_on_the_dsp_thread() {
        use sdrmm_test_support::assert_no_alloc;

        let (orders, commands) = mpsc::channel();
        let mut channels = hosted_channels();
        let mut monitors = hosted_monitors();
        let mut subbands = Box::new(Subbands::new(48_000.0));
        let mut tap = None;
        let mut network_tap = None;
        let mut history = None;
        let mut recording_publisher = None;
        let mut retirement = Reclaimer::new(Retired::release).expect("a reclaimer");
        let (host, _pcm) = crate::runtime::channel::tests::nfm_host();
        orders
            .send(DspCommand::AddChannel {
                id: 1,
                host,
                reset_state: false,
            })
            .expect("queued");
        assert_no_alloc("taking in a channel", || {
            drain_commands(
                &commands,
                &mut channels,
                &mut subbands,
                CommandSinks {
                    monitors: &mut monitors,
                    tap: &mut tap,
                    network_tap: &mut network_tap,
                    history: &mut history,
                    recording_publisher: &mut recording_publisher,
                },
                &mut retirement,
            );
        });
        assert_eq!(channels.len(), 1);
    }

    #[test]
    fn retiring_a_stalled_recording_never_waits_on_the_thread_that_retires_it() {
        use num_complex::Complex;
        use sdrmm_recorder::SigmfWriter;

        use crate::{
            recording::{create_tap, queue_depth, spawn_writer},
            runtime::DSP_BLOCK,
        };

        let rate = 48_000.0;
        let depth = queue_depth(rate);
        let directory = tempfile::tempdir().expect("scratch");
        let stem = directory.path().join("stalled");
        let writer = SigmfWriter::create(&stem, rate, 100e6, "test").expect("a writer");
        let (tap, position, messages, shared) = create_tap(rate);
        let samples = vec![Complex::new(0.5, -0.25); DSP_BLOCK];
        for index in 0..depth {
            assert!(tap.push(&samples, (index * DSP_BLOCK) as u64, 100e6));
        }
        let mut publisher = RecordingPublisher::new(rate).expect("a publisher");
        assert!(publisher.publish(&tap, &samples, (depth * DSP_BLOCK) as u64, 100e6));
        let retired = tap.clone();
        let (done, finished) = mpsc::channel();
        let retirement = std::thread::spawn(move || {
            Retired::Recording(Some(retired), Some(publisher)).release();
            let _ = done.send(());
        });
        let released = finished.recv_timeout(Duration::from_millis(500)).is_ok();
        let handle = spawn_writer(writer, messages, shared.clone()).expect("a writer thread");
        retirement.join().expect("retirement");
        drop(tap);
        drop(position);
        handle.join().expect("the writer finishes");
        assert!(released, "retirement waited on the stalled writer");
        assert_eq!(shared.error(), None);
        assert_eq!(shared.samples(), ((depth + 1) * DSP_BLOCK) as u64);
    }

    #[test]
    fn a_waker_with_no_thread_yet_is_a_no_op() {
        Waker::default().wake();
    }

    #[test]
    fn waking_releases_a_parked_thread_far_sooner_than_the_backstop() {
        let waker = Arc::new(Waker::default());
        let ready = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let parked = {
            let (waker, ready) = (waker.clone(), ready.clone());
            std::thread::spawn(move || {
                waker.adopt_current();
                ready.store(true, Ordering::Release);
                let start = Instant::now();
                std::thread::park_timeout(Duration::from_secs(30));
                let _ = tx.send(start.elapsed());
            })
        };
        while !ready.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
        waker.wake();
        let waited = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the parked thread was never released");
        parked.join().expect("thread panicked");
        assert!(
            waited < Duration::from_secs(5),
            "wake did not beat the backstop: {waited:?}"
        );
    }

    #[test]
    fn the_stall_counter_keeps_the_worst_gap_not_the_last() {
        let stalled = AtomicU64::new(0);
        let mut served = Instant::now() - Duration::from_millis(400);
        record_stall(&stalled, &mut served);
        let worst = stalled.load(Ordering::Relaxed);
        assert!(worst >= 400_000, "expected the 400 ms gap, got {worst} us");

        record_stall(&stalled, &mut served);
        assert_eq!(
            stalled.load(Ordering::Relaxed),
            worst,
            "a short gap must not erase the worst one"
        );
    }
}
