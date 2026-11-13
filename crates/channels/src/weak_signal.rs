use crate::registry::Native;
use sdrmm_wire::{HzRange, Modulation, SignalSignature};
use sdrmm_wire::{KindSpec, NumberSpec, SettingField};
mod model;
pub use model::*;
mod ftx;
mod wspr;

#[cfg(any(test, feature = "synth"))]
pub(crate) use ftx::{FT4, FT8, pack as pack_ftx, waveform as ftx_waveform};
#[cfg(any(test, feature = "synth"))]
pub(crate) use wspr::waveform as wspr_waveform;

use ftx::FtxDecoder;
use wspr::WsprDecoder;

use std::{
    sync::LazyLock,
    thread,
    time::{Duration, SystemTime},
};

use num_complex::Complex;
use rtrb::{Consumer, Producer, PushError, RingBuffer};
use sdrmm_dsp::{FirC, design_lowpass};
use sdrmm_wire::{
    ChannelDescriptor, ChannelParams, ChannelSettings, DecoderEvent, DecoderFamily, EventKind,
};

use crate::{
    ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, NativeChannel,
    check_input_rate,
};

const INPUT_RATE_HZ: f64 = 12_000.0;
const FILTER_TAPS: usize = 257;
const MIN_AUDIO_HZ: f32 = 50.0;
const MAX_AUDIO_HZ: f32 = 5_500.0;
const QUEUED_SLOTS: usize = 2;
const SPARE_SLOTS: usize = 3;
const WORKER_IDLE: Duration = Duration::from_millis(2);
const WSPR_POWER_DBM: [i32; 19] = [
    0, 3, 7, 10, 13, 17, 20, 23, 27, 30, 33, 37, 40, 43, 47, 50, 53, 57, 60,
];

static FT8_DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| {
    descriptor(
        "ft8",
        "FT8",
        "FT8 weak signal contacts",
        EventKind::named("ft8"),
    )
});
static FT4_DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| {
    descriptor(
        "ft4",
        "FT4",
        "FT4 fast weak signal contacts",
        EventKind::named("ft4"),
    )
});
static WSPR_DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| {
    descriptor(
        "wspr",
        "WSPR",
        "WSPR propagation beacons",
        EventKind::named("wspr"),
    )
});

fn descriptor(type_id: &str, name: &str, summary: &str, kind: EventKind) -> ChannelDescriptor {
    ChannelDescriptor {
        type_id: type_id.to_owned(),
        name: name.to_owned(),
        summary: summary.to_owned(),
        family: DecoderFamily::AMATEUR,
        bandwidth_hz: 3_200.0,
        input_rate_hz: INPUT_RATE_HZ,
        has_audio: false,
        emits: vec![kind],
        ..ChannelDescriptor::default()
    }
}

#[derive(Clone, Copy)]
enum Mode {
    Ft8,
    Ft4,
    Wspr,
}

impl Mode {
    fn kind(self) -> EventKind {
        EventKind::named(self.type_id())
    }

    fn slot_samples(self) -> usize {
        match self {
            Self::Ft8 => 180_000,
            Self::Ft4 => 90_000,
            Self::Wspr => 1_440_000,
        }
    }

    fn hop_samples(self) -> usize {
        match self {
            Self::Ft8 => 24_000,
            Self::Ft4 => 24_000,
            Self::Wspr => 60_000,
        }
    }

    fn type_id(self) -> &'static str {
        match self {
            Self::Ft8 => "ft8",
            Self::Ft4 => "ft4",
            Self::Wspr => "wspr",
        }
    }

    fn nominal_start_s(self) -> f64 {
        match self {
            Self::Ft8 | Self::Ft4 => 0.5,
            Self::Wspr => 1.0,
        }
    }

    /// The UTC period transmissions start on.
    fn period_s(self) -> f64 {
        match self {
            Self::Ft8 => 15.0,
            Self::Ft4 => 7.5,
            Self::Wspr => 120.0,
        }
    }

    /// Where a transmission that started `start_s` into a window captured at `window` sits on
    /// the UTC grid: its offset from the nominal start of the nearest period, and that period.
    fn on_the_grid(self, window: SystemTime, start_s: f32) -> Option<(f32, String)> {
        let window = window
            .duration_since(SystemTime::UNIX_EPOCH)
            .ok()?
            .as_secs_f64();
        let since_nominal = window + f64::from(start_s) - self.nominal_start_s();
        let period = (since_nominal / self.period_s()).round() * self.period_s();
        let start = jiff::Timestamp::from_millisecond((period * 1_000.0).round() as i64).ok()?;
        Some((
            (since_nominal - period) as f32,
            start.strftime("%Y-%m-%dT%H:%M:%SZ").to_string(),
        ))
    }

    /// The offset a decode reports: from the UTC grid when the window's capture time is known,
    /// else from the window's own nominal start.
    fn timing(self, window: Option<SystemTime>, start_s: f32) -> (f32, Option<String>) {
        match window.and_then(|window| self.on_the_grid(window, start_s)) {
            Some((offset, period)) => (offset, Some(period)),
            None => (start_s - self.nominal_start_s() as f32, None),
        }
    }
}

#[derive(Clone)]
struct Recent {
    text: String,
    audio_hz: f32,
    start_sample: i64,
}

struct Job {
    epoch: u64,
    window_start: u64,
    window_time: Option<SystemTime>,
    audio_low_hz: f32,
    audio_high_hz: f32,
    max_candidates: usize,
    samples: Vec<f32>,
}

/// A decode with the measurements the duplicate check reads, kept beside the event so the DSP
/// thread never parses an event payload back.
struct Decoded {
    text: String,
    audio_hz: f32,
    window_offset_s: f32,
    event: DecoderEvent,
}

struct Done {
    epoch: u64,
    window_start: u64,
    events: Vec<Decoded>,
    samples: Vec<f32>,
}

struct WeakSignal {
    mode: Mode,
    audio_low_hz: f32,
    audio_high_hz: f32,
    max_candidates: usize,
    audio: Vec<f32>,
    read: usize,
    window_start: u64,
    epoch: u64,
    queued: usize,
    skipped: u32,
    clock: Option<(u64, SystemTime)>,
    recent: Vec<Recent>,
    jobs: Producer<Job>,
    done: Consumer<Done>,
    spare: Vec<Vec<f32>>,
}

impl WeakSignal {
    fn new(mode: Mode, settings: &ChannelSettings) -> Result<Self, ChannelError> {
        let (low, high, candidates) = configured(mode, &settings.params)?;
        let (jobs, incoming) = RingBuffer::new(QUEUED_SLOTS);
        let (outgoing, done) = RingBuffer::new(QUEUED_SLOTS);
        thread::Builder::new()
            .name(format!("{}-decode", mode.type_id()))
            .spawn(move || run(mode, incoming, outgoing))
            .map_err(|error| {
                ChannelError::InvalidSettings(format!("Weak-signal worker: {error}"))
            })?;
        Ok(Self {
            mode,
            audio_low_hz: low,
            audio_high_hz: high,
            max_candidates: candidates,
            audio: Vec::with_capacity(2 * mode.slot_samples()),
            read: 0,
            window_start: 0,
            epoch: 0,
            queued: 0,
            skipped: 0,
            clock: None,
            recent: Vec::new(),
            jobs,
            done,
            spare: (0..SPARE_SLOTS)
                .map(|_| Vec::with_capacity(mode.slot_samples()))
                .collect(),
        })
    }

    fn apply(&mut self, settings: &ChannelSettings) -> Result<(), ChannelError> {
        let (low, high, candidates) = configured(self.mode, &settings.params)?;
        self.audio_low_hz = low;
        self.audio_high_hz = high;
        self.max_candidates = candidates;
        Ok(())
    }

    fn reset(&mut self) {
        self.audio.clear();
        self.read = 0;
        self.window_start = 0;
        self.epoch = self.epoch.wrapping_add(1);
        self.clock = None;
        self.recent.clear();
    }

    fn captured_at(&mut self, time: SystemTime) {
        let next = self.window_start + (self.audio.len() - self.read) as u64;
        self.clock = Some((next, time));
    }

    fn window_time(&self) -> Option<SystemTime> {
        let (index, time) = self.clock?;
        let seconds =
            |samples: u64| Duration::try_from_secs_f64(samples as f64 / INPUT_RATE_HZ).ok();
        if self.window_start >= index {
            time.checked_add(seconds(self.window_start - index)?)
        } else {
            time.checked_sub(seconds(index - self.window_start)?)
        }
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.compact(iq.len());
        self.audio.extend(iq.iter().map(|sample| {
            if sample.re.is_finite() {
                sample.re.clamp(-1.0, 1.0)
            } else {
                0.0
            }
        }));

        let slot = self.mode.slot_samples();
        let hop = self.mode.hop_samples();
        while self.audio.len() - self.read >= slot {
            self.submit(slot);
            self.read += hop;
            self.window_start += hop as u64;
            let oldest = self.window_start.saturating_sub((2 * slot) as u64) as i64;
            self.recent.retain(|item| item.start_sample >= oldest);
        }
        self.collect(out);
    }

    fn compact(&mut self, incoming: usize) {
        if self.audio.len() + incoming > self.audio.capacity() {
            self.audio.drain(..self.read);
            self.read = 0;
        }
    }

    fn submit(&mut self, slot: usize) {
        let Some(mut samples) = self.spare.pop() else {
            self.skipped = self.skipped.saturating_add(1);
            return;
        };
        samples.clear();
        samples.extend_from_slice(&self.audio[self.read..self.read + slot]);
        let job = Job {
            epoch: self.epoch,
            window_start: self.window_start,
            window_time: self.window_time(),
            audio_low_hz: self.audio_low_hz,
            audio_high_hz: self.audio_high_hz,
            max_candidates: self.max_candidates,
            samples,
        };
        match self.jobs.push(job) {
            Ok(()) => self.queued += 1,
            Err(PushError::Full(job)) => {
                self.skipped = self.skipped.saturating_add(1);
                self.spare.push(job.samples);
            }
        }
    }

    fn collect(&mut self, out: &mut ChannelOutputs) {
        while let Ok(done) = self.done.pop() {
            self.queued = self.queued.saturating_sub(1);
            if done.epoch == self.epoch {
                for event in done.events {
                    self.keep(done.window_start, event, out);
                }
            }
            self.spare.push(done.samples);
        }
    }

    fn keep(&mut self, window_start: u64, decoded: Decoded, out: &mut ChannelOutputs) {
        let start = window_start as i64
            + ((self.mode.nominal_start_s() + f64::from(decoded.window_offset_s)) * INPUT_RATE_HZ)
                .round() as i64;
        if self.is_duplicate(decoded.text, decoded.audio_hz, start) {
            return;
        }
        out.events.push(decoded.event);
    }

    #[cfg(test)]
    fn settle(&mut self, out: &mut ChannelOutputs) {
        while self.queued > 0 {
            self.collect(out);
            thread::sleep(WORKER_IDLE);
        }
    }

    fn is_duplicate(&mut self, text: String, audio_hz: f32, start_sample: i64) -> bool {
        let duplicate = self.recent.iter().any(|item| {
            item.text == text
                && (item.audio_hz - audio_hz).abs() < 3.0
                && (item.start_sample - start_sample).unsigned_abs() < INPUT_RATE_HZ as u64
        });
        if !duplicate {
            self.recent.push(Recent {
                text,
                audio_hz,
                start_sample,
            });
        }
        duplicate
    }
}

enum Worker {
    Ftx(Box<FtxDecoder>, EventKind),
    Wspr(Box<WsprDecoder>),
}

impl Worker {
    fn new(mode: Mode) -> Self {
        match mode {
            Mode::Ft8 => Self::Ftx(Box::new(FtxDecoder::new(&ftx::FT8)), mode.kind()),
            Mode::Ft4 => Self::Ftx(Box::new(FtxDecoder::new(&ftx::FT4)), mode.kind()),
            Mode::Wspr => Self::Wspr(Box::new(WsprDecoder::new())),
        }
    }

    fn decode(&mut self, job: &Job, mode: Mode) -> Vec<Decoded> {
        match self {
            Self::Ftx(decoder, kind) => decoder
                .decode(
                    &job.samples,
                    job.window_start,
                    job.audio_low_hz,
                    job.audio_high_hz,
                    job.max_candidates,
                )
                .into_iter()
                .map(|found| {
                    let (time_offset_s, slot) = mode.timing(job.window_time, found.start_s);
                    let message = WsjtMessage {
                        text: found.text,
                        snr_db: found.snr_db,
                        audio_hz: found.frequency_hz,
                        time_offset_s,
                        hard_errors: found.hard_errors,
                        slot,
                    };
                    Decoded {
                        event: DecoderEvent::tagged(kind.clone(), &message),
                        text: message.text,
                        audio_hz: message.audio_hz,
                        window_offset_s: found.start_s - mode.nominal_start_s() as f32,
                    }
                })
                .collect(),
            Self::Wspr(decoder) => decode_wspr(job, decoder, mode),
        }
    }
}

fn run(mode: Mode, mut input: Consumer<Job>, mut output: Producer<Done>) {
    let mut worker = Worker::new(mode);
    loop {
        if output.is_abandoned() {
            return;
        }
        let Ok(job) = input.pop() else {
            if input.is_abandoned() {
                return;
            }
            thread::sleep(WORKER_IDLE);
            continue;
        };
        let events = worker.decode(&job, mode);
        let mut done = Done {
            epoch: job.epoch,
            window_start: job.window_start,
            events,
            samples: job.samples,
        };
        while let Err(PushError::Full(held)) = output.push(done) {
            if output.is_abandoned() {
                return;
            }
            done = held;
            thread::sleep(WORKER_IDLE);
        }
    }
}

fn reported_by_a_transmitter(grid: Option<&str>, power_dbm: i32) -> bool {
    if !WSPR_POWER_DBM.contains(&power_dbm) {
        return false;
    }
    let Some(grid) = grid else {
        return true;
    };
    let locator = grid.as_bytes();
    if !matches!(locator.len(), 4 | 6) {
        return false;
    }
    let field = |byte: u8| (b'A'..=b'R').contains(&byte);
    let subsquare = |byte: u8| (b'A'..=b'X').contains(&byte);
    field(locator[0])
        && field(locator[1])
        && locator[2].is_ascii_digit()
        && locator[3].is_ascii_digit()
        && (locator.len() == 4 || (subsquare(locator[4]) && subsquare(locator[5])))
}

fn decode_wspr(job: &Job, decoder: &mut WsprDecoder, mode: Mode) -> Vec<Decoded> {
    decoder
        .decode(
            &job.samples,
            job.audio_low_hz,
            job.audio_high_hz,
            job.max_candidates,
        )
        .into_iter()
        .filter(|spot| {
            reported_by_a_transmitter(spot.message.grid.as_deref(), spot.message.power_dbm)
        })
        .map(|spot| {
            let (time_offset_s, slot) = mode.timing(job.window_time, spot.start_s);
            let window_offset_s = spot.start_s - mode.nominal_start_s() as f32;
            let spot = WsprSpot {
                text: spot.message.text,
                callsign: spot.message.callsign,
                grid: spot.message.grid,
                power_dbm: spot.message.power_dbm,
                snr_db: spot.snr_db,
                audio_hz: spot.frequency_hz,
                time_offset_s,
                drift_hz: spot.drift_hz,
                slot,
            };
            Decoded {
                event: DecoderEvent::of(&spot),
                text: spot.text,
                audio_hz: spot.audio_hz,
                window_offset_s,
            }
        })
        .collect()
}

fn configured(mode: Mode, params: &ChannelParams) -> Result<(f32, f32, usize), ChannelError> {
    if params.type_id() != mode.type_id() {
        return Err(ChannelError::InvalidSettings(format!(
            "weak-signal channel got {} params",
            params.type_id()
        )));
    }
    let search = match mode {
        Mode::Ft8 | Mode::Ft4 => {
            wsjt_config(&params.parse().map_err(ChannelError::InvalidSettings)?)
        }
        Mode::Wspr => wspr_config(&params.parse().map_err(ChannelError::InvalidSettings)?),
    };
    validate_passband(search)?;
    Ok(search)
}

fn wsjt_config(params: &WsjtParams) -> (f32, f32, usize) {
    (
        params.audio_low_hz,
        params.audio_high_hz,
        usize::from(params.max_candidates),
    )
}

fn wspr_config(params: &WsprParams) -> (f32, f32, usize) {
    (
        params.audio_low_hz,
        params.audio_high_hz,
        usize::from(params.max_candidates),
    )
}

fn validate_passband((low, high, candidates): (f32, f32, usize)) -> Result<(), ChannelError> {
    if !(low.is_finite()
        && high.is_finite()
        && (MIN_AUDIO_HZ..high).contains(&low)
        && high <= MAX_AUDIO_HZ)
    {
        return Err(ChannelError::InvalidSettings(format!(
            "audio search band must satisfy {MIN_AUDIO_HZ} <= low < high <= {MAX_AUDIO_HZ} Hz, got {low}..{high}"
        )));
    }
    if candidates == 0 {
        return Err(ChannelError::InvalidSettings(
            "max_candidates must be greater than zero".to_owned(),
        ));
    }
    Ok(())
}

fn search_band((low, high, _): (f32, f32, usize)) -> (f64, f64) {
    (f64::from(low), f64::from(high))
}

fn search_filter(search: (f32, f32, usize)) -> Result<ChannelFilter, ChannelError> {
    validate_passband(search)?;
    let (low, high) = search_band(search);
    let half_width = (high - low) / 2.0 / INPUT_RATE_HZ;
    let center = (high + low) / 2.0 / INPUT_RATE_HZ;
    Ok(ChannelFilter::Sideband(FirC::from_lowpass(
        &design_lowpass(FILTER_TAPS, half_width),
        center,
    )))
}

macro_rules! channel {
    ($name:ident, $mode:expr, $descriptor:ident) => {
        pub struct $name(WeakSignal);

        impl NativeChannel for $name {
            fn descriptor() -> &'static ChannelDescriptor {
                &$descriptor
            }
            fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
                check_input_rate(ctx, &$descriptor)?;
                Ok(Self(WeakSignal::new($mode, &settings)?))
            }
        }

        impl ChannelRx for $name {
            fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
                self.0.apply(&settings)
            }

            fn retuned(&mut self) {
                self.0.reset();
            }

            fn captured_at(&mut self, time: SystemTime) {
                self.0.captured_at(time);
            }

            fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
                self.0.process(iq, out);
            }

            fn skipped(&self) -> u32 {
                self.0.skipped
            }
        }

        impl $name {
            #[cfg(test)]
            pub(crate) fn settle(&mut self, out: &mut ChannelOutputs) {
                self.0.settle(out);
            }
        }
    };
}

channel!(Ft8Channel, Mode::Ft8, FT8_DESCRIPTOR);
channel!(Ft4Channel, Mode::Ft4, FT4_DESCRIPTOR);
channel!(WsprChannel, Mode::Wspr, WSPR_DESCRIPTOR);

const FT8_SPOTS: &[HzRange] = &[
    HzRange::spot_khz(1_840.0, 2.0),
    HzRange::spot_khz(3_573.0, 2.0),
    HzRange::spot_khz(7_074.0, 2.0),
    HzRange::spot_khz(10_136.0, 2.0),
    HzRange::spot_khz(14_074.0, 2.0),
    HzRange::spot_khz(18_100.0, 2.0),
    HzRange::spot_khz(21_074.0, 2.0),
    HzRange::spot_khz(24_915.0, 2.0),
    HzRange::spot_khz(28_074.0, 2.0),
    HzRange::spot_khz(50_313.0, 2.0),
];

fn ft8_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "FT8",
            &[Modulation::Fsk8],
            HzRange::new(30.0, 90.0),
            "eight tones at 6.25 baud in a 50 Hz slot",
        )
        .symbol_rate(HzRange::about(6.25, 0.15))
        .on(FT8_SPOTS),
    ]
}

pub(crate) fn ft8_decoder() -> Native<WsjtParams> {
    Native::new::<Ft8Channel>(|params| search_band(wsjt_config(params)))
        .filter(|params| search_filter(wsjt_config(params)))
        .home(14_074_000.0)
        .signatures(ft8_signatures)
        .form(wsjt_form)
        .kinds(ft8_kinds)
}

pub(crate) fn wsjt_form() -> Vec<SettingField> {
    vec![
        SettingField::new("audio_low_hz", "From", NumberSpec::hz(50.0, 5_450.0, 10.0))
            .title("Lowest USB audio frequency searched"),
        SettingField::new("audio_high_hz", "To", NumberSpec::hz(100.0, 5_500.0, 10.0))
            .title("Highest USB audio frequency searched"),
        SettingField::new(
            "max_candidates",
            "Candidates",
            NumberSpec::new(1.0, 1_000.0, 1.0),
        )
        .title("Maximum synchronized signals tried per decode pass"),
    ]
}

fn ft8_kinds() -> Vec<KindSpec> {
    vec![ft8_kind()]
}

const FT4_SPOTS: &[HzRange] = &[
    HzRange::spot_khz(3_575.0, 2.0),
    HzRange::spot_khz(7_047.5, 2.0),
    HzRange::spot_khz(10_140.0, 2.0),
    HzRange::spot_khz(14_080.0, 2.0),
    HzRange::spot_khz(18_104.0, 2.0),
    HzRange::spot_khz(21_140.0, 2.0),
    HzRange::spot_khz(24_919.0, 2.0),
    HzRange::spot_khz(28_180.0, 2.0),
];

fn ft4_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "FT4",
            &[Modulation::Fsk4],
            HzRange::new(60.0, 130.0),
            "four tones at 20.8 baud in a 90 Hz slot",
        )
        .symbol_rate(HzRange::about(20.83, 0.15))
        .on(FT4_SPOTS),
    ]
}

pub(crate) fn ft4_decoder() -> Native<WsjtParams> {
    Native::new::<Ft4Channel>(|params| search_band(wsjt_config(params)))
        .filter(|params| search_filter(wsjt_config(params)))
        .home(14_080_000.0)
        .signatures(ft4_signatures)
        .form(wsjt_form)
        .kinds(ft4_kinds)
}

fn ft4_kinds() -> Vec<KindSpec> {
    vec![ft4_kind()]
}

const WSPR_SPOTS: &[HzRange] = &[
    HzRange::spot_khz(1_836.6, 1.0),
    HzRange::spot_khz(3_568.6, 1.0),
    HzRange::spot_khz(7_038.6, 1.0),
    HzRange::spot_khz(10_138.7, 1.0),
    HzRange::spot_khz(14_095.6, 1.0),
    HzRange::spot_khz(18_104.6, 1.0),
    HzRange::spot_khz(21_094.6, 1.0),
    HzRange::spot_khz(24_924.6, 1.0),
    HzRange::spot_khz(28_124.6, 1.0),
];

fn wspr_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "WSPR",
            &[Modulation::Fsk4, Modulation::Carrier],
            HzRange::new(0.0, 20.0),
            "a 6 Hz wide four-tone beacon on a WSPR dial frequency",
        )
        .on(WSPR_SPOTS),
    ]
}

pub(crate) fn wspr_decoder() -> Native<WsprParams> {
    Native::new::<WsprChannel>(|params| search_band(wspr_config(params)))
        .filter(|params| search_filter(wspr_config(params)))
        .home(14_095_600.0)
        .signatures(wspr_signatures)
        .form(wsjt_form)
        .kinds(wspr_kinds)
}

fn wspr_kinds() -> Vec<KindSpec> {
    vec![wspr_kind()]
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::{
        synth,
        testutil::{realtime_budget, settings},
    };

    fn wsjt(event: &DecoderEvent, kind: &str) -> Option<WsjtMessage> {
        if event.kind == kind {
            event.payload()
        } else {
            None
        }
    }

    macro_rules! decode {
        ($channel:expr, $iq:expr) => {{
            let mut channel = $channel;
            let mut out = ChannelOutputs::default();
            channel.process($iq, &mut out);
            channel.settle(&mut out);
            out.events
        }};
    }

    #[test]
    fn ft8_fixture_decodes_to_its_message_and_measurements() {
        let events = decode!(
            Ft8Channel::new(
                ChannelCtx {
                    input_rate: INPUT_RATE_HZ,
                },
                settings(ChannelParams::new("ft8", &WsjtParams::default()).expect("ft8 settings")),
            )
            .unwrap(),
            &synth::weak_signal::ft8_slot("W1AW", "FN42", 1_500.0).expect("a synthetic signal")
        );
        let message = events
            .iter()
            .filter_map(|event| wsjt(event, "ft8"))
            .find(|message| message.text.contains("W1AW"));
        let message = message.expect("FT8 fixture decode");
        assert!((message.audio_hz - 1_500.0).abs() < 10.0);
        assert!(message.time_offset_s.abs() < 0.2);
    }

    #[test]
    fn ft4_fixture_decodes_to_its_message_and_measurements() {
        let events = decode!(
            Ft4Channel::new(
                ChannelCtx {
                    input_rate: INPUT_RATE_HZ,
                },
                settings(ChannelParams::new("ft4", &WsjtParams::default()).expect("ft4 settings")),
            )
            .unwrap(),
            &synth::weak_signal::ft4_slot("JA1ABC", "PM95", 1_000.0).expect("a synthetic signal")
        );
        let message = events
            .iter()
            .filter_map(|event| wsjt(event, "ft4"))
            .find(|message| message.text.contains("JA1ABC"));
        let message = message.expect("FT4 fixture decode");
        assert!((message.audio_hz - 1_000.0).abs() < 20.0);
        assert!(message.time_offset_s.abs() < 0.2);
    }

    #[test]
    fn wspr_fixture_decodes_to_a_spot() {
        let events = decode!(
            WsprChannel::new(
                ChannelCtx {
                    input_rate: INPUT_RATE_HZ,
                },
                settings(ChannelParams::of(&WsprParams::default()).expect("wspr settings")),
            )
            .unwrap(),
            &synth::weak_signal::wspr_slot("K1ABC", "FN42", 37, 1_500.0)
                .expect("a synthetic signal")
        );
        let spot = events
            .iter()
            .filter_map(|event| event.read::<WsprSpot>())
            .find(|spot| spot.callsign == "K1ABC");
        let spot = spot.expect("WSPR fixture decode");
        assert_eq!(spot.grid.as_deref(), Some("FN42"));
        assert_eq!(spot.power_dbm, 37);
        assert!((spot.audio_hz - 1_500.0).abs() < 3.0);
    }

    #[test]
    fn windows_stay_aligned_after_the_audio_buffer_compacts() {
        let hop = Mode::Ft8.hop_samples();
        let mut iq = vec![Complex::new(0.0, 0.0); 9 * hop];
        iq.extend(
            synth::weak_signal::ft8_slot("W1AW", "FN42", 1_500.0).expect("a synthetic signal"),
        );
        let mut channel = Ft8Channel::new(
            ChannelCtx {
                input_rate: INPUT_RATE_HZ,
            },
            settings(ChannelParams::new("ft8", &WsjtParams::default()).expect("ft8 settings")),
        )
        .unwrap();
        let capacity = channel.0.audio.capacity();
        let mut out = ChannelOutputs::default();
        for block in iq.chunks(hop) {
            channel.process(block, &mut out);
            channel.settle(&mut out);
        }
        assert_eq!(channel.0.audio.capacity(), capacity);
        assert_eq!(channel.skipped(), 0);
        let one_hop_late = (hop as f64 / INPUT_RATE_HZ) as f32;
        assert!(
            out.events
                .iter()
                .filter_map(|event| wsjt(event, "ft8"))
                .any(|message| {
                    message.text.contains("W1AW")
                        && (message.time_offset_s - one_hop_late).abs() < 0.2
                })
        );
    }

    #[test]
    fn windows_the_worker_cannot_take_are_counted_as_skipped() {
        let hop = Mode::Ft8.hop_samples();
        let mut iq = vec![Complex::new(0.0, 0.0); Mode::Ft8.slot_samples() + 20 * hop];
        synth::add_noise(&mut iq, 7, 0.1);
        let mut channel = Ft8Channel::new(
            ChannelCtx {
                input_rate: INPUT_RATE_HZ,
            },
            settings(ChannelParams::new("ft8", &WsjtParams::default()).expect("ft8 settings")),
        )
        .unwrap();
        let mut out = ChannelOutputs::default();
        channel.process(&iq, &mut out);
        assert!(channel.skipped() >= 21 - SPARE_SLOTS as u32);
    }

    #[test]
    fn a_slot_decode_never_blocks_the_sample_path() {
        let iq = synth::weak_signal::ft8_slot("W1AW", "FN42", 1_500.0).expect("a synthetic signal");
        let mut channel = Ft8Channel::new(
            ChannelCtx {
                input_rate: INPUT_RATE_HZ,
            },
            settings(ChannelParams::new("ft8", &WsjtParams::default()).expect("ft8 settings")),
        )
        .unwrap();
        let mut out = ChannelOutputs::default();
        let started = Instant::now();
        for block in iq.chunks(4_096) {
            channel.process(block, &mut out);
        }
        let spent = started.elapsed().as_secs_f64();
        let slot_seconds = iq.len() as f64 / INPUT_RATE_HZ;
        assert!(
            spent < realtime_budget(slot_seconds / 10.0),
            "feeding {slot_seconds:.1} s of audio held the sample path for {spent:.3} s"
        );
        channel.settle(&mut out);
        assert!(
            out.events
                .iter()
                .filter_map(|event| wsjt(event, "ft8"))
                .any(|message| message.text.contains("W1AW")),
            "{:?}",
            out.events
        );
    }

    #[test]
    fn a_spot_no_transmitter_could_have_sent_is_not_reported() {
        for (grid, power_dbm) in [
            (Some("A000AA"), 63),
            (Some("BWB9H "), 15),
            (Some("JN46"), 15),
            (Some("JN46!"), 30),
            (Some("SN46"), 30),
        ] {
            assert!(
                !reported_by_a_transmitter(grid, power_dbm),
                "{grid:?} at {power_dbm} dBm passed"
            );
        }
        for (grid, power_dbm) in [(Some("JN46"), 30), (Some("JN49GR"), 23), (None, 37)] {
            assert!(
                reported_by_a_transmitter(grid, power_dbm),
                "{grid:?} at {power_dbm} dBm was refused"
            );
        }
    }

    fn busy_slot_with_quiet_tail() -> Vec<Complex<f32>> {
        const FIXTURE: &[u8] = include_bytes!("../../../fixtures/ft8_20m_busy_12k.sigmf-data");
        let mut iq: Vec<Complex<f32>> = FIXTURE
            .as_chunks::<8>()
            .0
            .iter()
            .map(|sample| {
                Complex::new(
                    f32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]),
                    f32::from_le_bytes([sample[4], sample[5], sample[6], sample[7]]),
                )
            })
            .collect();
        iq.resize(iq.len() + 4 * INPUT_RATE_HZ as usize, Complex::default());
        iq
    }

    fn decode_ft8(iq: &[Complex<f32>]) -> Vec<String> {
        let mut channel = Ft8Channel::new(
            ChannelCtx {
                input_rate: INPUT_RATE_HZ,
            },
            settings(ChannelParams::new("ft8", &WsjtParams::default()).expect("ft8 settings")),
        )
        .unwrap();
        let mut out = ChannelOutputs::default();
        for block in iq.chunks(4_096) {
            channel.process(block, &mut out);
        }
        channel.settle(&mut out);
        out.events
            .into_iter()
            .filter_map(|event| wsjt(&event, "ft8").map(|message| message.text))
            .collect()
    }

    fn with_noise(iq: &[Complex<f32>], snr_db: f32, mut state: u64) -> Vec<Complex<f32>> {
        let power = iq.iter().map(|s| s.re * s.re).sum::<f32>() / iq.len() as f32;
        let sigma = (power / 10f32.powf(snr_db / 10.0)).sqrt();
        let mut gaussian = || {
            (0..12)
                .map(|_| {
                    state = state
                        .wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(1_442_695_040_888_963_407);
                    (state >> 40) as f32 / (1u64 << 24) as f32
                })
                .sum::<f32>()
                - 6.0
        };
        iq.iter()
            .map(|s| Complex::new(s.re + sigma * gaussian(), 0.0))
            .collect()
    }

    #[test]
    fn noise_on_a_busy_band_invents_no_messages() {
        let heard: Vec<&str> = PUBLISHED
            .iter()
            .copied()
            .chain(["OT4B <...> -19", "WB2QJ ES3AT KO18"])
            .collect();
        let slot = busy_slot_with_quiet_tail();
        for (snr_db, seed) in [(0.0, 1), (0.0, 2), (-5.0, 3), (-5.0, 4)] {
            let texts = decode_ft8(&with_noise(&slot, snr_db, seed));
            let invented: Vec<&String> = texts
                .iter()
                .filter(|text| !heard.contains(&text.as_str()))
                .collect();
            assert!(
                invented.is_empty(),
                "{snr_db} dB, seed {seed}: {invented:?}"
            );
        }
    }

    const PUBLISHED: [&str; 20] = [
        "VK4BLE OH8JK R-17",
        "RK6AH JH1AJT -05",
        "PA3EPP SP8NFO KN09",
        "RV6K RU3XL -13",
        "SQ8OHR UA9LL MO27",
        "ET3RFG/R IN3ADG -23",
        "CQ F4FSY JN25",
        "JR5MJS OH8NW 73",
        "SV1GN RK6AUV LN05",
        "PB5DX EI3CTB IO63",
        "CQ IZ1ANK JN33",
        "NT6Q OH8GDU -17",
        "CQ DL1UDO JO31",
        "VK4BLE OH1EDK -20",
        "CQ JA OH1LWZ KP11",
        "<...> ON7EE JO10",
        "CQ DG0OFT JO50",
        "CQ UB3AQS KO85",
        "G1XJM HA7JIV JN97",
        "SP7XIF JA2GQT -15",
    ];

    #[test]
    fn a_recorded_slot_reads_the_band_the_reference_decoder_published() {
        let mut channel = Ft8Channel::new(
            ChannelCtx {
                input_rate: INPUT_RATE_HZ,
            },
            settings(ChannelParams::new("ft8", &WsjtParams::default()).expect("ft8 settings")),
        )
        .unwrap();
        let mut out = ChannelOutputs::default();
        let mut texts = Vec::new();
        let take = |out: &mut ChannelOutputs, texts: &mut Vec<String>| {
            for event in out.events.drain(..) {
                let Some(message) = wsjt(&event, "ft8") else {
                    panic!("the FT8 channel emitted something else")
                };
                texts.push(message.text);
            }
        };
        for block in busy_slot_with_quiet_tail().chunks(4_096) {
            out.reset();
            channel.process(block, &mut out);
            take(&mut out, &mut texts);
        }
        channel.settle(&mut out);
        take(&mut out, &mut texts);

        let missing: Vec<&str> = PUBLISHED
            .iter()
            .filter(|wanted| !texts.iter().any(|text| text == *wanted))
            .copied()
            .collect();
        assert_eq!(
            missing,
            ["CQ UB3AQS KO85"],
            "the recording's published decodes changed, read {texts:?}"
        );
        assert!(
            texts.iter().all(|text| text.is_ascii() && !text.is_empty()),
            "{texts:?}"
        );
    }

    /// A station sending `text` at 1 234 Hz in every other FT8 slot, at each period's SNR (none
    /// when it is silent), in white noise.
    fn fading_station(text: &str, snrs: &[Option<f32>]) -> Vec<Complex<f32>> {
        const RATE: usize = 12_000;
        const SIGMA: f32 = 0.05;
        let lead = RATE;
        let mut audio = vec![0.0f32; lead + snrs.len() * 30 * RATE + 15 * RATE];
        let mut state = 0x5eed_u64;
        let mut uniform = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((state >> 11) as f64 / (1u64 << 53) as f64).max(1e-12)
        };
        for sample in &mut audio {
            let (u, v) = (uniform(), uniform());
            *sample = SIGMA * ((-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()) as f32;
        }
        let wave = ftx_waveform(&FT8, pack_ftx(text).expect("packs"), 1_234.0);
        for (period, snr) in snrs.iter().enumerate() {
            let Some(snr) = snr else { continue };
            let amplitude = SIGMA * (2.0 * 10f32.powf(snr / 10.0) * 2_500.0 / 6_000.0).sqrt();
            let start = lead + period * 30 * RATE + 8 * RATE / 10;
            for (sample, value) in audio[start..].iter_mut().zip(&wave) {
                *sample += amplitude * value;
            }
        }
        audio
            .into_iter()
            .map(|sample| Complex::new(sample, 0.0))
            .collect()
    }

    fn messages(iq: &[Complex<f32>], captured: Option<SystemTime>) -> Vec<WsjtMessage> {
        let mut channel = Ft8Channel::new(
            ChannelCtx {
                input_rate: INPUT_RATE_HZ,
            },
            settings(ChannelParams::new("ft8", &WsjtParams::default()).expect("ft8 settings")),
        )
        .unwrap();
        if let Some(time) = captured {
            channel.captured_at(time);
        }
        let mut out = ChannelOutputs::default();
        for block in iq.chunks(Mode::Ft8.hop_samples()) {
            channel.process(block, &mut out);
            channel.settle(&mut out);
        }
        out.events
            .iter()
            .filter_map(|event| wsjt(event, "ft8"))
            .collect()
    }

    fn heard(iq: &[Complex<f32>], text: &str) -> usize {
        let texts: Vec<String> = messages(iq, None)
            .into_iter()
            .map(|message| message.text)
            .collect();
        assert!(texts.iter().all(|heard| heard == text), "{texts:?}");
        texts.len()
    }

    #[test]
    fn a_known_capture_time_puts_decodes_on_the_utc_grid() {
        let first_slot: jiff::Timestamp = "2026-11-07T14:59:45Z".parse().unwrap();
        let captured = SystemTime::from(first_slot) - Duration::from_secs(1);
        let iq = fading_station("CQ K1ABC FN42", &[Some(-10.0), Some(-10.0)]);
        let timed = messages(&iq, Some(captured));
        let slots: Vec<&str> = timed.iter().filter_map(|m| m.slot.as_deref()).collect();
        assert_eq!(slots, ["2026-11-07T14:59:45Z", "2026-11-07T15:00:15Z"]);
        assert!(
            timed.iter().all(|m| (m.time_offset_s - 0.3).abs() < 0.05),
            "{timed:?}"
        );
        assert!(messages(&iq, None).iter().all(|m| m.slot.is_none()));
    }

    #[test]
    fn a_station_that_fades_is_still_heard_where_it_was() {
        let text = "CQ K1ABC FN42";
        assert_eq!(
            heard(
                &fading_station(text, &[None, Some(-23.0), Some(-23.0)]),
                text
            ),
            0
        );
        assert_eq!(
            heard(
                &fading_station(text, &[Some(-14.0), Some(-23.0), Some(-23.0)]),
                text
            ),
            3
        );
    }

    #[test]
    fn noise_where_a_station_was_invents_nothing() {
        let text = "K1ABC W9XYZ R-07";
        assert_eq!(
            heard(
                &fading_station(text, &[Some(-14.0), None, None, None]),
                text
            ),
            1
        );
    }
}
