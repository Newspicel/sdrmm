use std::{
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use num_complex::Complex;
use sdrmm_device::{
    DeviceError, LaneEvent, LaneMark, MarkPoster, RxSink, SdrDevice, SinkItem, lock,
};
use sdrmm_dsp::{
    array_sync::{
        BinSolver, Boxcar, COARSE_FRAME, COARSE_LAGS, CoarseSearch, NOISE_CLIPPED_MAX,
        NOISE_COHERENCE_MIN, NOISE_PURITY_MIN, NOISE_XCORR_MIN, coarse_decimation,
    },
    doa::{DOMINANT_WITHIN_DB, NOISE_ABOVE_DB, dominance_count, mdl_count},
    linalg::{CMat, Eigen, HermitianEigen},
    xcorr::XCorr,
};
use sdrmm_wire::{AgcSetting, DeviceSettings, GainKind, GainValue};

use super::{Lane, MAX_RATE_HZ, NOISE_SOURCE_PIN, lanes};
use crate::{
    RtlSdrDevice,
    dongle::{Catalog, Dongle, GAINS, IN_FLIGHT_SAMPLES},
};

type C32 = Complex<f32>;

const OPEN_WAIT: Duration = Duration::from_secs(15);
const OPEN_POLL: Duration = Duration::from_millis(250);

const CLIP_LEVEL: f32 = 0.999;
const RATE_HZ: u32 = 2_400_000;
const SPREAD_CENTER_HZ: u32 = 433_920_000;
const SPREAD_RATES_HZ: [u32; 3] = [1_024_000, 2_400_000, 2_560_000];
const SPREAD_STARTS: usize = 20;
const PHASE_FREQUENCIES_HZ: [u32; 6] = [
    100_000_000,
    433_920_000,
    868_000_000,
    1_090_000_000,
    1_300_000_000,
    1_700_000_000,
];
const CAPPED_RATE_HZ: f64 = 2_880_000.0;
const COUNT_CENTER_HZ: u32 = 433_920_000;
const ENVELOPE_SUB: usize = 1_024;
const DECAY_GAINS_TENTHS: [i32; 2] = [0, 297];
const DECAY_QUIET: Duration = Duration::from_millis(400);
const DECAY_ON: Duration = Duration::from_millis(300);
const DROP_RATIO: f32 = 4.0;
const RETURN_RATIOS: [f32; 2] = [2.0, 1.25];
const COUNT_GAINS_TENTHS: [i32; 3] = [0, 197, 297];
const COUNT_LEN: usize = 262_144;
const COUNT_OFFSET_HZ: f64 = 100_000.0;
const COUNT_BANDS: [(&str, usize); 3] = [("37.5 kHz", 64), ("300 kHz", 8), ("2.4 MHz", 1)];
const SOLVE_LEN: usize = 65_536;
const BIN_LEN: usize = 1_024;
const FINE_FRAME: usize = 16_384;
const FLOWING_BLOCKS: u64 = 8;
const CAPTURE_WAIT: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_millis(5);
const CLIPPING_WINDOW: Duration = Duration::from_millis(300);
const CLIPPING_SETTLE: Duration = Duration::from_millis(100);

const CLIPPING_FREQUENCIES_HZ: [u32; 9] = [
    30_000_000,
    100_000_000,
    433_920_000,
    700_000_000,
    868_000_000,
    1_000_000_000,
    1_090_000_000,
    1_300_000_000,
    1_700_000_000,
];

fn hardware_dir() -> PathBuf {
    let binary = std::env::current_exe().expect("the test binary");
    binary
        .ancestors()
        .nth(3)
        .expect("the target directory")
        .join("hardware")
}

fn write_csv(name: &str, header: &str, rows: &[String]) {
    let dir = hardware_dir();
    std::fs::create_dir_all(&dir).expect("target/hardware");
    let path = dir.join(format!("{name}.csv"));
    let mut text = format!("{header}\n");
    for row in rows {
        text.push_str(row);
        text.push('\n');
    }
    std::fs::write(&path, text).expect("the CSV is written");
    println!("wrote {}", path.display());
}

fn whole_bank(catalog: &Catalog) -> Result<Vec<(usize, Lane)>, String> {
    let listed: Vec<_> = catalog.listings().cloned().collect();
    let found: Vec<(usize, Lane)> = lanes(&listed)
        .into_iter()
        .enumerate()
        .filter_map(|(index, lane)| lane.map(|lane| (index, lane)))
        .collect();
    let first = found
        .first()
        .ok_or_else(|| "no KrakenSDR lane".to_owned())?;
    let mut held: Vec<(usize, Lane)> = found
        .iter()
        .filter(|(_, lane)| lane.id == first.1.id)
        .cloned()
        .collect();
    held.sort_by_key(|(_, lane)| lane.lane);
    if !held
        .iter()
        .map(|(_, lane)| lane.lane)
        .eq(0..first.1.model.lanes())
    {
        return Err(format!("{} is missing a lane", first.1.label()));
    }
    Ok(held)
}

fn first_lane(catalog: &Catalog) -> Result<usize, String> {
    whole_bank(catalog)?
        .first()
        .map(|(index, _)| *index)
        .ok_or_else(|| "no KrakenSDR lane".to_owned())
}

fn waited<T>(mut attempt: impl FnMut() -> Result<T, String>) -> T {
    let started = Instant::now();
    loop {
        match attempt() {
            Ok(value) => return value,
            Err(error) if started.elapsed() < OPEN_WAIT => {
                println!("waiting for the KrakenSDR: {error}");
                std::thread::sleep(OPEN_POLL);
            }
            Err(error) => panic!("the KrakenSDR does not open: {error}"),
        }
    }
}

pub(super) struct Bank {
    pub(super) lanes: Vec<RtlSdrDevice>,
}

impl Bank {
    pub(super) fn open() -> Self {
        waited(Self::attempt)
    }

    fn attempt() -> Result<Self, String> {
        let catalog = Catalog::scan().map_err(|error| error.to_string())?;
        let held = whole_bank(&catalog)?;
        let mut lanes = Vec::with_capacity(held.len());
        for (index, lane) in held {
            let dongle = catalog.open(index).map_err(|error| error.to_string())?;
            lanes.push(
                RtlSdrDevice::from_dongle(dongle, Some(lane)).map_err(|error| error.to_string())?,
            );
        }
        Ok(Self { lanes })
    }

    pub(super) fn apply(&mut self, settings: &DeviceSettings) -> Result<(), DeviceError> {
        let mut outcome = Ok(());
        for lane in &mut self.lanes {
            if let Err(error) = lane.apply(settings)
                && outcome.is_ok()
            {
                outcome = Err(error);
            }
        }
        outcome
    }

    pub(super) fn set_noise_source(&mut self, on: bool) -> Result<(), DeviceError> {
        self.lanes
            .first_mut()
            .ok_or_else(|| DeviceError::NotFound("an empty bank".to_owned()))?
            .set_noise_source(on)
    }

    pub(super) fn rx_start(&mut self, sinks: Vec<RxSink>) -> Result<(), DeviceError> {
        if sinks.len() != self.lanes.len() {
            return Err(DeviceError::Unsupported(format!(
                "this bank has {} lanes, got {} sinks",
                self.lanes.len(),
                sinks.len()
            )));
        }
        for (lane, sink) in self.lanes.iter_mut().zip(sinks) {
            lane.rx_start(vec![sink])?;
        }
        Ok(())
    }

    pub(super) fn rx_stop(&mut self) {
        for lane in &mut self.lanes {
            lane.rx_stop();
        }
    }

    pub(super) fn settings(&self) -> &DeviceSettings {
        self.lanes
            .first()
            .map_or_else(|| unreachable!("a bank has lanes"), SdrDevice::settings)
    }

    pub(super) fn control(&self) -> MutexGuard<'_, Dongle> {
        self.lanes.first().map_or_else(
            || unreachable!("a bank has lanes"),
            |lane| lane.radio.lock(),
        )
    }
}

fn raw_control() -> Dongle {
    waited(|| {
        let catalog = Catalog::scan().map_err(|error| error.to_string())?;
        let index = first_lane(&catalog)?;
        catalog.open(index).map_err(|error| error.to_string())
    })
}

pub(super) fn gain(tenths: i32) -> Vec<GainValue> {
    vec![GainValue::new(GainKind::Tuner, f64::from(tenths) / 10.0)]
}

pub(super) fn tuned(rate_hz: u32, center_hz: u32, tenths: i32) -> DeviceSettings {
    DeviceSettings {
        sample_rate: Some(f64::from(rate_hz)),
        center_hz: Some(f64::from(center_hz)),
        agc: Some(AgcSetting::switched(false)),
        gains: gain(tenths),
        ..DeviceSettings::default()
    }
}

pub(super) fn gain_only(tenths: i32) -> DeviceSettings {
    DeviceSettings {
        gains: gain(tenths),
        ..DeviceSettings::default()
    }
}

struct Want {
    from: u64,
    len: usize,
    at: u64,
    boxcar: Boxcar,
    out: Vec<Vec<C32>>,
    clipped: usize,
    broken: bool,
}

impl Want {
    fn new(from: u64, len: usize, factor: usize) -> Self {
        Self {
            from,
            len,
            at: from,
            boxcar: Boxcar::new(1, factor),
            out: vec![Vec::with_capacity(len)],
            clipped: 0,
            broken: false,
        }
    }

    fn full(&self) -> bool {
        self.out[0].len() >= self.len
    }

    fn raw_left(&self) -> usize {
        let raw = (self.len * self.boxcar.factor()) as u64;
        raw.saturating_sub(self.at - self.from) as usize
    }

    fn take(&mut self, samples: &[C32], index: u64) {
        let end = index + samples.len() as u64;
        if end <= self.from || self.full() || self.broken {
            return;
        }
        let skip = self.from.saturating_sub(index) as usize;
        if index + skip as u64 != self.at {
            self.broken = true;
            return;
        }
        let used = self.raw_left().min(samples.len() - skip);
        let slice = &samples[skip..skip + used];
        self.clipped += slice
            .iter()
            .filter(|sample| sample.re.abs() >= CLIP_LEVEL || sample.im.abs() >= CLIP_LEVEL)
            .count();
        if self.boxcar.push(&[slice], &mut self.out).is_err() {
            self.broken = true;
        }
        self.at += used as u64;
        self.out[0].truncate(self.len);
    }
}

#[derive(Default)]
struct Envelope {
    acc: f64,
    filled: usize,
    start: u64,
    powers: Vec<(u64, f32)>,
}

impl Envelope {
    fn take(&mut self, samples: &[C32], index: u64) {
        for (offset, sample) in samples.iter().enumerate() {
            if self.filled == 0 {
                self.start = index + offset as u64;
                self.acc = 0.0;
            }
            self.acc += f64::from(sample.norm_sqr());
            self.filled += 1;
            if self.filled == ENVELOPE_SUB {
                self.powers
                    .push((self.start, (self.acc / ENVELOPE_SUB as f64) as f32));
                self.filled = 0;
            }
        }
    }
}

#[derive(Default)]
struct Tape {
    next: u64,
    blocks: u64,
    gaps: u64,
    want: Option<Want>,
    envelope: Option<Envelope>,
    events: Vec<(u64, LaneEvent)>,
    fatal: Vec<String>,
}

impl Tape {
    fn take(&mut self, item: SinkItem<'_>) {
        match item {
            SinkItem::Samples { samples, index } => {
                if self.blocks > 0 && index != self.next {
                    self.gaps += 1;
                }
                self.next = index + samples.len() as u64;
                self.blocks += 1;
                if let Some(want) = &mut self.want {
                    want.take(samples, index);
                }
                if let Some(envelope) = &mut self.envelope {
                    envelope.take(samples, index);
                }
            }
            SinkItem::Event(event) => self.events.push((self.blocks, event)),
        }
    }
}

type Deck = Arc<Mutex<Tape>>;

fn sinks(lanes: usize) -> (Vec<RxSink>, Vec<Deck>) {
    (0..lanes)
        .map(|_| {
            let deck = Deck::default();
            let writer = deck.clone();
            let fatal = deck.clone();
            let sink = RxSink::with_items(
                move |item| lock(&writer).take(item),
                move |error| lock(&fatal).fatal.push(error.to_string()),
            );
            (sink, deck)
        })
        .unzip()
}

fn wait_until(what: &str, timeout: Duration, mut done: impl FnMut() -> bool) -> Duration {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < timeout, "timed out waiting for {what}");
        std::thread::sleep(POLL);
    }
    started.elapsed()
}

fn wait_flowing(decks: &[Deck]) {
    wait_until("every lane to stream", CAPTURE_WAIT, || {
        decks.iter().all(|deck| lock(deck).blocks >= FLOWING_BLOCKS)
    });
}

struct Taken {
    lanes: Vec<Vec<C32>>,
    clipped: Vec<f64>,
}

fn record(decks: &[Deck], factor: usize, len: usize) -> Taken {
    let latest = decks.iter().map(|deck| lock(deck).next).max().unwrap_or(0);
    let from = latest + IN_FLIGHT_SAMPLES;
    for deck in decks {
        lock(deck).want = Some(Want::new(from, len, factor));
    }
    wait_until("the lanes to fill a capture", CAPTURE_WAIT, || {
        decks.iter().all(|deck| {
            lock(deck)
                .want
                .as_ref()
                .is_some_and(|want| want.full() || want.broken)
        })
    });
    let mut taken = Taken {
        lanes: Vec::with_capacity(decks.len()),
        clipped: Vec::with_capacity(decks.len()),
    };
    for (lane, deck) in decks.iter().enumerate() {
        let want = lock(deck).want.take().expect("an armed capture");
        assert!(!want.broken, "lane {lane} lost samples inside a capture");
        taken
            .clipped
            .push(want.clipped as f64 / (want.len * want.boxcar.factor()) as f64);
        taken
            .lanes
            .push(want.out.into_iter().next().unwrap_or_default());
    }
    taken
}

#[derive(Clone, Debug)]
struct Alignment {
    factor: usize,
    lags: Vec<i64>,
    coarse_db: Vec<f32>,
    fine_coherence: Vec<f32>,
}

impl Alignment {
    fn reach(&self) -> usize {
        self.lags
            .iter()
            .map(|lag| lag.unsigned_abs() as usize)
            .max()
            .unwrap_or(0)
            + 2 * self.factor
    }

    fn spread(&self) -> i64 {
        let high = self.lags.iter().copied().max().unwrap_or(0);
        let low = self.lags.iter().copied().min().unwrap_or(0);
        high - low
    }

    fn views<'a>(&self, lanes: &'a [Vec<C32>], len: usize) -> Vec<&'a [C32]> {
        let base = self.reach() as i64;
        lanes
            .iter()
            .zip(&self.lags)
            .map(|(lane, lag)| {
                let start = (base + lag) as usize;
                &lane[start..start + len]
            })
            .collect()
    }
}

fn coarse(decks: &[Deck], rate_hz: u32) -> Result<Alignment, String> {
    let factor = coarse_decimation(f64::from(rate_hz), COARSE_LAGS);
    let mut search = CoarseSearch::new(COARSE_FRAME, COARSE_LAGS);
    let taken = record(decks, factor, search.span());
    let mut alignment = Alignment {
        factor,
        lags: vec![0],
        coarse_db: vec![f32::INFINITY],
        fine_coherence: Vec::new(),
    };
    for (lane, samples) in taken.lanes.iter().enumerate().skip(1) {
        let found = search
            .lag(&taken.lanes[0], samples)
            .map_err(|error| format!("lane {lane}: {error}"))?;
        alignment.lags.push(found.lag * factor as i64);
        alignment.coarse_db.push(found.peak_db);
    }
    Ok(alignment)
}

fn align(decks: &[Deck], rate_hz: u32) -> Result<Alignment, String> {
    let mut alignment = coarse(decks, rate_hz)?;
    let taken = record(decks, 1, 2 * alignment.reach() + FINE_FRAME);
    let views = alignment.views(&taken.lanes, FINE_FRAME);
    let mut xcorr = XCorr::new(FINE_FRAME);
    let mut refined = Vec::with_capacity(views.len());
    for view in &views {
        let estimate = xcorr.estimate(views[0], view);
        refined.push(estimate.delay_samples.round() as i64);
        alignment.fine_coherence.push(estimate.coherence);
    }
    for (lag, residual) in alignment.lags.iter_mut().zip(refined) {
        *lag += residual;
    }
    Ok(alignment)
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values.get(values.len() / 2).copied().unwrap_or(f64::NAN)
}

fn clipped_fraction(control: &mut Dongle) -> f64 {
    let stream = control.start_stream().expect("stream");
    let started = Instant::now();
    let (mut clipped, mut total) = (0u64, 0u64);
    while started.elapsed() < CLIPPING_WINDOW {
        if let Ok(block) = stream.recv_timeout(Duration::from_millis(100)) {
            if started.elapsed() < CLIPPING_SETTLE {
                continue;
            }
            clipped += block.iter().filter(|byte| matches!(byte, 0 | 255)).count() as u64;
            total += block.len() as u64;
        }
    }
    clipped as f64 / total.max(1) as f64
}

#[test]
#[ignore = "requires an idle KrakenSDR; measures how hard its noise source drives each gain"]
fn kraken_noise_clipping_levels() {
    let mut bank = Bank::open();
    bank.apply(&tuned(RATE_HZ, SPREAD_CENTER_HZ, 0))
        .expect("tune");
    bank.set_noise_source(true).expect("noise on");
    let mut rows = Vec::new();
    for freq in CLIPPING_FREQUENCIES_HZ {
        let mut printed = Vec::new();
        for &tenths in GAINS {
            let fraction = {
                let mut control = bank.control();
                control.set_center(freq).expect("tune");
                control.set_manual_gain(tenths).expect("gain");
                clipped_fraction(&mut control)
            };
            let db = f64::from(tenths) / 10.0;
            rows.push(format!("{},{db:.1},{fraction:.5}", freq / 1_000_000));
            printed.push(format!("{db:.1}dB:{fraction:.4}"));
        }
        println!("{} MHz {}", freq / 1_000_000, printed.join(" "));
    }
    write_csv(
        "kraken_noise_clipping_levels",
        "freq_mhz,gain_db,clipped_fraction",
        &rows,
    );
}

fn spread_at(bank: &mut Bank, rate: u32, rows: &mut Vec<String>) -> String {
    bank.apply(&tuned(rate, SPREAD_CENTER_HZ, 0)).expect("tune");
    bank.set_noise_source(true).expect("noise on");
    let factor = coarse_decimation(f64::from(rate), COARSE_LAGS);
    let reach_ms = (COARSE_LAGS * factor) as f64 / f64::from(rate) * 1e3;
    let mut spreads = Vec::with_capacity(SPREAD_STARTS);
    for start in 0..SPREAD_STARTS {
        let (sinks, decks) = sinks(bank.lanes.len());
        bank.rx_start(sinks).expect("the bank starts");
        wait_flowing(&decks);
        let alignment =
            align(&decks, rate).unwrap_or_else(|error| panic!("{rate} S/s start {start}: {error}"));
        bank.rx_stop();
        for (lane, lag) in alignment.lags.iter().enumerate() {
            let lag_ms = *lag as f64 / f64::from(rate) * 1e3;
            assert!(
                lag_ms.abs() < reach_ms,
                "{rate} S/s start {start} lane {lane}: {lag_ms:.3} ms"
            );
            rows.push(format!(
                "{rate},{start},{lane},{lag},{lag_ms:.4},{:.1},{:.3}",
                alignment.coarse_db[lane], alignment.fine_coherence[lane]
            ));
        }
        spreads.push(alignment.spread() as f64 / f64::from(rate) * 1e3);
    }
    let low = spreads.iter().copied().fold(f64::INFINITY, f64::min);
    let high = spreads.iter().copied().fold(0.0, f64::max);
    let middle = median(&mut spreads);
    format!(
        "{rate} S/s: spread min {low:.3} ms, median {middle:.3} ms, max {high:.3} ms, reach {reach_ms:.0} ms"
    )
}

#[test]
#[ignore = "requires an idle KrakenSDR; restarts the bank 60 times with the noise source on"]
fn kraken_start_spread() {
    let mut bank = Bank::open();
    let mut rows = Vec::new();
    let mut summary = Vec::new();
    for rate in SPREAD_RATES_HZ {
        let line = spread_at(&mut bank, rate, &mut rows);
        println!("{line}");
        summary.push(line);
    }
    write_csv(
        "kraken_start_spread",
        "rate_hz,start,lane,lag_samples,lag_ms,coarse_peak_db,fine_coherence",
        &rows,
    );
    write_csv("kraken_start_spread_summary", "summary", &summary);
}

#[derive(Clone, Copy, Debug)]
struct LaneSolve {
    phase_deg: f64,
    gain_db: f64,
    coherence: f32,
    delay_frac: f32,
    xcorr: f32,
}

struct Step {
    tenths: i32,
    clipped: f64,
    purity: f32,
    lanes: Vec<LaneSolve>,
    error: Option<String>,
}

impl Step {
    fn db(&self) -> f64 {
        f64::from(self.tenths) / 10.0
    }
}

fn solve_step(decks: &[Deck], alignment: &Alignment, tenths: i32) -> Step {
    let taken = record(decks, 1, 2 * alignment.reach() + SOLVE_LEN);
    let clipped = taken.clipped.iter().copied().fold(0.0, f64::max);
    let views = alignment.views(&taken.lanes, SOLVE_LEN);
    let mut xcorr = XCorr::new(FINE_FRAME);
    let pairs: Vec<f32> = views
        .iter()
        .map(|view| xcorr.estimate(views[0], view).coherence)
        .collect();
    let mut solver = BinSolver::new(views.len(), BIN_LEN);
    let solved = solver.solve(&views, NOISE_PURITY_MIN, 0.0);
    Step {
        tenths,
        clipped,
        purity: solved.as_ref().map_or(0.0, |solution| solution.purity),
        lanes: solved.as_ref().map_or_else(
            |_| Vec::new(),
            |solution| {
                solution
                    .lanes
                    .iter()
                    .zip(&pairs)
                    .map(|(lane, xcorr)| LaneSolve {
                        phase_deg: f64::from(lane.phase_rad).to_degrees(),
                        gain_db: 20.0 * f64::from(lane.gain).log10(),
                        coherence: lane.coherence,
                        delay_frac: lane.delay_frac,
                        xcorr: *xcorr,
                    })
                    .collect()
            },
        ),
        error: solved.err().map(|error| error.to_string()),
    }
}

fn wrap_deg(degrees: f64) -> f64 {
    (degrees + 180.0).rem_euclid(360.0) - 180.0
}

fn phase_extremes(steps: &[Step], lane: usize) -> ((f64, f64), (f64, f64)) {
    let Some(first) = steps.first().and_then(|step| step.lanes.get(lane)) else {
        return ((0.0, 0.0), (0.0, 0.0));
    };
    let moved: Vec<(f64, f64)> = steps
        .iter()
        .filter_map(|step| {
            let solve = step.lanes.get(lane)?;
            Some((wrap_deg(solve.phase_deg - first.phase_deg), step.db()))
        })
        .collect();
    let low = moved
        .iter()
        .copied()
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .unwrap_or_default();
    let high = moved
        .iter()
        .copied()
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .unwrap_or_default();
    (low, high)
}

fn gain_sweep(bank: &mut Bank, freq: u32) -> Vec<Step> {
    bank.apply(&tuned(RATE_HZ, freq, 0)).expect("tune");
    bank.set_noise_source(true).expect("noise on");
    let (sinks, decks) = sinks(bank.lanes.len());
    bank.rx_start(sinks).expect("the bank starts");
    wait_flowing(&decks);
    let alignment = align(&decks, RATE_HZ).unwrap_or_else(|error| panic!("{freq} Hz: {error}"));
    let mut steps = Vec::new();
    for &tenths in GAINS {
        bank.apply(&gain_only(tenths)).expect("gain");
        steps.push(solve_step(&decks, &alignment, tenths));
    }
    bank.rx_stop();
    for (lane, deck) in decks.iter().enumerate() {
        let tape = lock(deck);
        assert_eq!(tape.gaps, 0, "lane {lane} lost samples during the sweep");
        assert!(tape.fatal.is_empty(), "lane {lane}: {:?}", tape.fatal);
    }
    steps
}

fn step_rows(freq: u32, step: &Step, rows: &mut Vec<String>) {
    let mhz = freq / 1_000_000;
    let db = step.db();
    if let Some(error) = &step.error {
        rows.push(format!(
            "{mhz},{db:.1},{:.5},{:.4},,,,,,,{error}",
            step.clipped, step.purity
        ));
    }
    for (lane, solve) in step.lanes.iter().enumerate() {
        rows.push(format!(
            "{mhz},{db:.1},{:.5},{:.4},{lane},{:.2},{:.3},{:.4},{:.4},{:.4},",
            step.clipped,
            step.purity,
            solve.phase_deg,
            solve.gain_db,
            solve.coherence,
            solve.delay_frac,
            solve.xcorr
        ));
    }
    let phases: Vec<String> = step
        .lanes
        .iter()
        .map(|solve| {
            format!(
                "{:7.2}/{:.3}/{:.3}",
                solve.phase_deg, solve.coherence, solve.xcorr
            )
        })
        .collect();
    println!(
        "{mhz} MHz {db:4.1} dB clip {:.4} purity {:.3}: {} {}",
        step.clipped,
        step.purity,
        phases.join(" "),
        step.error.as_deref().unwrap_or("")
    );
}

fn assert_solvable(freq: u32, steps: &[Step]) {
    for step in steps {
        let at = format!("{freq} Hz {} dB", step.db());
        assert!(step.error.is_none(), "{at}: {:?}", step.error);
        assert!(
            step.clipped <= f64::from(NOISE_CLIPPED_MAX),
            "{at}: {} clipped",
            step.clipped
        );
        assert!(
            step.purity >= NOISE_PURITY_MIN,
            "{at}: purity {}",
            step.purity
        );
        for (lane, solve) in step.lanes.iter().enumerate() {
            assert!(
                solve.coherence >= NOISE_COHERENCE_MIN,
                "{at} lane {lane}: coherence {}",
                solve.coherence
            );
            assert!(
                solve.xcorr >= NOISE_XCORR_MIN,
                "{at} lane {lane}: xcorr {}",
                solve.xcorr
            );
        }
    }
}

fn phase_summary(freq: u32, steps: &[Step], lanes: usize) -> Vec<String> {
    let lowest = |pick: fn(&LaneSolve) -> f64| {
        steps
            .iter()
            .flat_map(|step| step.lanes.iter().skip(1).map(pick))
            .fold(f64::INFINITY, f64::min)
    };
    let clipped = steps.iter().map(|step| step.clipped).fold(0.0, f64::max);
    let purity = steps
        .iter()
        .map(|step| step.purity)
        .fold(f32::INFINITY, f32::min);
    let mut lines = vec![format!(
        "{} MHz: most clipped {clipped:.3}, lowest purity {purity:.3}, lowest coherence {:.3}, lowest xcorr {:.3}",
        freq / 1_000_000,
        lowest(|solve| f64::from(solve.coherence)),
        lowest(|solve| f64::from(solve.xcorr))
    )];
    for lane in 1..lanes {
        let ((low, low_db), (high, high_db)) = phase_extremes(steps, lane);
        lines.push(format!(
            "{} MHz lane {lane}: phase {low:+.2} deg at {low_db:.1} dB to {high:+.2} deg at {high_db:.1} dB, from 0 dB",
            freq / 1_000_000
        ));
    }
    lines
}

#[test]
#[ignore = "requires an idle KrakenSDR; solves lane phase on the noise source at every gain step"]
fn kraken_phase_vs_gain() {
    let mut bank = Bank::open();
    let mut rows = Vec::new();
    let mut summary = Vec::new();
    let mut sweeps = Vec::new();
    for freq in PHASE_FREQUENCIES_HZ {
        let steps = gain_sweep(&mut bank, freq);
        for step in &steps {
            step_rows(freq, step, &mut rows);
        }
        for line in phase_summary(freq, &steps, bank.lanes.len()) {
            println!("{line}");
            summary.push(line);
        }
        sweeps.push((freq, steps));
    }
    drop(bank);
    write_csv(
        "kraken_phase_vs_gain",
        "freq_mhz,gain_db,clipped_fraction,purity,lane,phase_deg,gain_db_rel,coherence,delay_frac,xcorr_coherence,error",
        &rows,
    );
    write_csv("kraken_phase_vs_gain_summary", "summary", &summary);
    for (freq, steps) in &sweeps {
        assert_solvable(*freq, steps);
    }
}

#[test]
#[ignore = "requires an idle KrakenSDR"]
fn kraken_rate_cap() {
    let mut bank = Bank::open();
    let refused = bank.apply(&DeviceSettings {
        sample_rate: Some(CAPPED_RATE_HZ),
        ..DeviceSettings::default()
    });
    let Err(DeviceError::Unsupported(message)) = refused else {
        panic!("a lane runs at most 2.56 MS/s, got {refused:?}");
    };
    println!("{message}");
    bank.apply(&DeviceSettings {
        sample_rate: Some(MAX_RATE_HZ),
        ..DeviceSettings::default()
    })
    .expect("2.56 MS/s is the cap");
    assert_eq!(bank.settings().sample_rate, Some(MAX_RATE_HZ));
    write_csv(
        "kraken_rate_cap",
        "requested_hz,outcome",
        &[
            format!("{CAPPED_RATE_HZ},{message}"),
            format!("{MAX_RATE_HZ},accepted"),
        ],
    );
}

fn switch_noise_off() -> Result<(), String> {
    let catalog = Catalog::scan().map_err(|error| error.to_string())?;
    let index = first_lane(&catalog)?;
    catalog
        .open(index)
        .and_then(|control| control.set_pin(NOISE_SOURCE_PIN, false))
        .map_err(|error| error.to_string())
}

struct NoiseOffOnExit;

impl Drop for NoiseOffOnExit {
    fn drop(&mut self) {
        if let Err(error) = switch_noise_off() {
            eprintln!("the noise source may still be on: {error}");
        }
    }
}

fn noise_pin() -> bool {
    raw_control()
        .pin_high(NOISE_SOURCE_PIN)
        .expect("read the noise pin")
}

fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default()
}

fn noise_pin_after(scenario: impl FnOnce() + Send + 'static) -> (bool, Option<String>) {
    let panicked = std::thread::spawn(scenario)
        .join()
        .err()
        .map(|payload| panic_text(payload.as_ref()));
    (noise_pin(), panicked)
}

fn streaming_with_noise() -> Bank {
    let mut bank = Bank::open();
    bank.set_noise_source(true).expect("noise on");
    let (sinks, decks) = sinks(bank.lanes.len());
    bank.rx_start(sinks).expect("the bank starts");
    wait_flowing(&decks);
    assert!(
        bank.control()
            .pin_high(NOISE_SOURCE_PIN)
            .expect("read the noise pin")
    );
    bank
}

const PLANNED_PANIC: &str = "a test failed with the noise source on";

#[test]
#[ignore = "requires an idle KrakenSDR; leaves the noise source on in three ways and checks it"]
fn kraken_noise_source_is_off_on_every_exit() {
    let _off = NoiseOffOnExit;
    let found = noise_pin();
    let (left_on, control_panic) = noise_pin_after(|| {
        raw_control()
            .set_pin(NOISE_SOURCE_PIN, true)
            .expect("noise on");
    });
    assert_eq!(control_panic, None);
    assert!(left_on, "a pin left on must read as on after a reopen");
    switch_noise_off().expect("noise off");
    assert!(!noise_pin(), "the pin switches off again");
    let (dropped, dropped_panic) = noise_pin_after(|| {
        let mut bank = Bank::open();
        bank.set_noise_source(true).expect("noise on");
    });
    let (streaming, streaming_panic) = noise_pin_after(|| drop(streaming_with_noise()));
    let (panicked, planned) = noise_pin_after(|| {
        let _bank = streaming_with_noise();
        panic!("{PLANNED_PANIC}");
    });
    write_csv(
        "kraken_noise_source_is_off_on_every_exit",
        "exit,noise_on_after",
        &[
            format!("left by the last user,{found}"),
            format!("reopened without switching off,{left_on}"),
            format!("dropped,{dropped}"),
            format!("dropped while streaming,{streaming}"),
            format!("panicked while streaming,{panicked}"),
        ],
    );
    assert_eq!(dropped_panic, None);
    assert_eq!(streaming_panic, None);
    assert_eq!(planned.as_deref(), Some(PLANNED_PANIC));
    assert!(!found && !dropped && !streaming && !panicked);
}

fn eigenvalues_desc(snapshots: &[Vec<C32>]) -> Vec<f32> {
    let order = snapshots.len();
    let len = snapshots.iter().map(Vec::len).min().unwrap_or(0);
    let mut matrix = CMat::zeros(order).expect("a covariance");
    for row in 0..order {
        for col in 0..order {
            let sum: C32 = (0..len)
                .map(|n| snapshots[row][n] * snapshots[col][n].conj())
                .sum();
            matrix.set(row, col, sum / len.max(1) as f32);
        }
    }
    let mut eigen = Eigen::new();
    HermitianEigen::new(order)
        .expect("an eigen solver")
        .solve(&matrix, &mut eigen)
        .expect("eigenvalues");
    let mut values = eigen.values().to_vec();
    values.sort_by(|a, b| b.total_cmp(a));
    values
}

fn without_dc(view: &[C32]) -> Vec<C32> {
    let mean = view.iter().sum::<C32>() / view.len().max(1) as f32;
    view.iter().map(|sample| sample - mean).collect()
}

fn band_limited(view: &[C32], rate_hz: u32, factor: usize) -> Vec<C32> {
    let step = -std::f64::consts::TAU * COUNT_OFFSET_HZ / f64::from(rate_hz);
    let mixed: Vec<C32> = without_dc(view)
        .iter()
        .enumerate()
        .map(|(n, sample)| sample * C32::from_polar(1.0, (step * n as f64) as f32))
        .collect();
    let mut boxcar = Boxcar::new(1, factor);
    let mut out = vec![Vec::with_capacity(mixed.len() / factor)];
    boxcar.push(&[&mixed], &mut out).expect("one lane");
    out.into_iter().next().unwrap_or_default()
}

fn decibels(ratio: f32) -> f32 {
    10.0 * ratio.max(f32::MIN_POSITIVE).log10()
}

struct Count {
    band: &'static str,
    values: Vec<f32>,
    snapshots: usize,
}

impl Count {
    fn dominance(&self) -> usize {
        dominance_count(&self.values)
    }

    fn mdl(&self) -> usize {
        mdl_count(&self.values, self.snapshots as f64)
    }

    fn row(&self, tenths: i32, noise: bool) -> String {
        let top = self.values[0];
        let spread: Vec<String> = self
            .values
            .iter()
            .map(|value| format!("{:.1}", decibels(value / top)))
            .collect();
        let above: Vec<String> = (0..self.values.len() - 1)
            .map(|k| {
                let rest = &self.values[k + 1..];
                let floor = rest.iter().sum::<f32>() / rest.len() as f32;
                format!("{:.1}", decibels(self.values[k] / floor))
            })
            .collect();
        format!(
            "{:.1},{noise},{},{},{},{},{}",
            f64::from(tenths) / 10.0,
            self.band,
            spread.join(" "),
            above.join(" "),
            self.dominance(),
            self.mdl()
        )
    }
}

fn counts(decks: &[Deck], alignment: &Alignment) -> Vec<Count> {
    let taken = record(decks, 1, 2 * alignment.reach() + COUNT_LEN);
    let views = alignment.views(&taken.lanes, COUNT_LEN);
    COUNT_BANDS
        .iter()
        .map(|&(band, factor)| {
            let limited: Vec<Vec<C32>> = views
                .iter()
                .map(|view| band_limited(view, RATE_HZ, factor))
                .collect();
            Count {
                band,
                snapshots: limited[0].len(),
                values: eigenvalues_desc(&limited),
            }
        })
        .collect()
}

#[test]
#[ignore = "requires an idle KrakenSDR; counts sources on receiver noise and on the noise source"]
fn kraken_source_count_on_the_noise_source() {
    let mut bank = Bank::open();
    bank.apply(&tuned(RATE_HZ, COUNT_CENTER_HZ, 0))
        .expect("tune");
    bank.set_noise_source(true).expect("noise on");
    let (sinks, decks) = sinks(bank.lanes.len());
    bank.rx_start(sinks).expect("the bank starts");
    wait_flowing(&decks);
    let alignment = align(&decks, RATE_HZ).expect("lanes align on the noise source");
    let mut rows = Vec::new();
    let mut seen = Vec::new();
    for tenths in COUNT_GAINS_TENTHS {
        bank.apply(&gain_only(tenths)).expect("gain");
        for noise in [true, false] {
            bank.set_noise_source(noise).expect("noise switch");
            for count in counts(&decks, &alignment) {
                let row = count.row(tenths, noise);
                println!("{row}");
                rows.push(row);
                seen.push((tenths, noise, count.band, count.dominance()));
            }
        }
    }
    bank.rx_stop();
    drop(bank);
    write_csv(
        "kraken_source_count_on_the_noise_source",
        &format!(
            "gain_db,noise_on,band,eigenvalues_db,above_rest_db,dominance,mdl,thresholds {NOISE_ABOVE_DB} dB above and {DOMINANT_WITHIN_DB} dB within"
        ),
        &rows,
    );
    for (tenths, noise, band, dominance) in seen {
        let expected = usize::from(noise);
        if noise && band == COUNT_BANDS[2].0 {
            continue;
        }
        assert_eq!(
            dominance,
            expected,
            "{} dB noise {noise} {band}: dominance counts {dominance}",
            f64::from(tenths) / 10.0
        );
    }
}

fn noise_marks(tape: &Tape) -> (Option<u64>, Option<u64>) {
    let mut on = None;
    let mut off = None;
    for (_, event) in &tape.events {
        if let LaneEvent::Mark {
            at,
            mark: LaneMark::NoiseSource { on: switched, .. },
        } = event
        {
            if *switched {
                on = Some(*at);
            } else {
                off = Some(*at);
            }
        }
    }
    (on, off)
}

fn median_of(values: impl Iterator<Item = f32>) -> f32 {
    let mut values: Vec<f32> = values.collect();
    values.sort_by(f32::total_cmp);
    values.get(values.len() / 2).copied().unwrap_or(0.0)
}

struct Decay {
    baseline: f32,
    on: f32,
    drop_after_off: i64,
    returns: Vec<Option<u64>>,
    tail: Vec<(u64, f32)>,
}

fn decay_of(tape: &Tape) -> Decay {
    let (Some(on_at), Some(off_at)) = noise_marks(tape) else {
        panic!("the lane saw no noise switch marks");
    };
    let powers = &tape.envelope.as_ref().expect("an envelope").powers;
    let baseline = median_of(
        powers
            .iter()
            .filter(|(start, _)| *start + ENVELOPE_SUB as u64 <= on_at)
            .map(|(_, power)| *power),
    );
    let settled = on_at + IN_FLIGHT_SAMPLES;
    let on = median_of(
        powers
            .iter()
            .filter(|(start, _)| *start >= settled && *start < off_at)
            .map(|(_, power)| *power),
    );
    let after: Vec<(u64, f32)> = powers
        .iter()
        .copied()
        .filter(|(start, _)| *start >= off_at)
        .collect();
    let dropped = after
        .iter()
        .find(|(_, power)| *power < on / DROP_RATIO)
        .map_or(off_at, |(start, _)| *start);
    let returns = RETURN_RATIOS
        .iter()
        .map(|ratio| {
            after
                .iter()
                .find(|(start, power)| *start > dropped && *power < ratio * baseline)
                .map(|(start, _)| start - dropped)
        })
        .collect();
    Decay {
        baseline,
        on,
        drop_after_off: dropped as i64 - off_at as i64,
        returns,
        tail: after
            .into_iter()
            .filter(|(start, _)| *start >= dropped)
            .take(24)
            .map(|(start, power)| (start - dropped, power / baseline))
            .collect(),
    }
}

fn switch_noise(bank: &mut Bank, posters: &[MarkPoster], on: bool) {
    bank.set_noise_source(on).expect("noise switch");
    let mark = LaneMark::NoiseSource {
        on,
        in_flight: IN_FLIGHT_SAMPLES,
    };
    for poster in posters {
        poster.post(mark).expect("a free mark slot");
    }
}

#[test]
#[ignore = "requires an idle KrakenSDR; measures how fast the noise source dies away"]
fn kraken_noise_source_decay() {
    let mut bank = Bank::open();
    let mut rows = Vec::new();
    let mut tails = Vec::new();
    for tenths in DECAY_GAINS_TENTHS {
        bank.apply(&tuned(RATE_HZ, SPREAD_CENTER_HZ, tenths))
            .expect("tune");
        let (sinks, decks) = sinks(bank.lanes.len());
        let posters: Vec<MarkPoster> = sinks.iter().map(RxSink::mark_poster).collect();
        for deck in &decks {
            lock(deck).envelope = Some(Envelope::default());
        }
        bank.rx_start(sinks).expect("the bank starts");
        wait_flowing(&decks);
        std::thread::sleep(DECAY_QUIET);
        switch_noise(&mut bank, &posters, true);
        std::thread::sleep(DECAY_ON);
        switch_noise(&mut bank, &posters, false);
        std::thread::sleep(DECAY_QUIET);
        bank.rx_stop();
        for (lane, deck) in decks.iter().enumerate() {
            let decay = decay_of(&lock(deck));
            let returns: Vec<String> = decay
                .returns
                .iter()
                .map(|samples| samples.map_or_else(|| "never".to_owned(), |n| n.to_string()))
                .collect();
            rows.push(format!(
                "{:.1},{lane},{:.1},{:.1},{},{}",
                f64::from(tenths) / 10.0,
                10.0 * decay.baseline.log10(),
                10.0 * decay.on.log10(),
                decay.drop_after_off,
                returns.join(",")
            ));
            if lane == 0 {
                let tail: Vec<String> = decay
                    .tail
                    .iter()
                    .map(|(after, ratio)| format!("{after}:{:.1}", 10.0 * ratio.log10()))
                    .collect();
                tails.push(format!(
                    "{:.1} dB lane 0 tail (samples after the drop: dB over baseline) {}",
                    f64::from(tenths) / 10.0,
                    tail.join(" ")
                ));
            }
        }
    }
    drop(bank);
    for line in rows.iter().chain(&tails) {
        println!("{line}");
    }
    write_csv(
        "kraken_noise_source_decay",
        "gain_db,lane,baseline_dbfs,on_dbfs,drop_after_off_samples,within_3db_after_drop_samples,within_1db_after_drop_samples",
        &rows,
    );
    write_csv("kraken_noise_source_decay_tail", "tail", &tails);
}
