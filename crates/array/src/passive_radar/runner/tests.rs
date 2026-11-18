use std::{
    f64::consts::TAU,
    ops::Range,
    sync::{
        Arc, Mutex, MutexGuard, OnceLock, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use num_complex::Complex;
use sdrmm_test_support::assert_no_alloc;
use sdrmm_wire::{
    ArrayGeometry, ArrayTuningMode, Coherence, DfBearing, RadarUpdate, RangeDopplerOwned,
    SurfaceFrame, TrackChange, Winding,
};

use super::{
    worker::{COMMANDS, Shared},
    *,
};
use crate::passive_radar::{
    CafError, CfarParams, CpiJob, CubeOut, LiveParams, PassiveRadarParams, ReferenceCleaning, plan,
};
use crate::{
    ArrayBlock, CalView, CorrectionView, DedicatedRunner, LaneBuffer, OutputSlots, Pose,
    ProcessorAction, ProcessorOutput, Reading, Settings,
};

/// The largest block a host hands its runner.
const HOST_BLOCK: usize = 16_384 + 3_968;

/// A host without accelerators that leaves thread priority alone.
struct BareHost;

impl RunnerHost for BareHost {
    fn raise(&self) {}

    fn caf(
        &self,
        _: &crate::kernel::CafPlan<'_>,
    ) -> Result<Box<dyn crate::kernel::CafKernel>, String> {
        Err("no accelerator".to_owned())
    }
}

fn host() -> Arc<dyn RunnerHost> {
    Arc::new(BareHost)
}

type C32 = Complex<f32>;

const RATE: f64 = 500_000.0;
const CENTER_HZ: f64 = 100e6;
const ELEMENTS: usize = 3;
const BLOCK: usize = 16_384;
const SCENE: usize = 2_400_000;
const BASE_NS: u64 = 1_780_000_000_000_000_000;
const ECHO_DELAY: usize = 60;
const ECHO_HZ: f64 = 30.0;
const PATIENCE: Duration = Duration::from_secs(30);
const PACE: Duration = Duration::from_millis(1);

fn params() -> PassiveRadarParams {
    PassiveRadarParams {
        cpi_ms: 200,
        aoa: false,
        reference: ReferenceCleaning::Off,
        gpu: GpuUse::Off,
        ..PassiveRadarParams::default()
    }
}

fn stage_of(params: &PassiveRadarParams, center_hz: f64) -> StagePlan {
    let ctx = crate::passive_radar::RadarCtx {
        sample_rate: RATE,
        center_hz,
        elements: ELEMENTS,
        positions_m: Vec::new(),
        manifold: None,
        tuned_together: true,
    };
    plan(&ctx, params).expect("plan")
}

fn xorshift(seed: u64) -> impl FnMut() -> f64 {
    let mut state = seed | 1;
    move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        ((state >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }
}

fn scene() -> &'static [Vec<C32>] {
    static LANES: OnceLock<Vec<Vec<C32>>> = OnceLock::new();
    LANES.get_or_init(|| {
        let mut uniform = xorshift(3);
        let mut phase = 0.0f64;
        let mut drive = 0.0f64;
        let reference: Vec<C32> = (0..SCENE)
            .map(|_| {
                drive = 0.995 * drive + 0.1 * (uniform() - 0.5);
                phase += drive;
                C32::from_polar(1.0, phase as f32)
            })
            .collect();
        let mut lanes = vec![reference.clone()];
        for element in 1..ELEMENTS {
            let gain = C32::from_polar(0.9, element as f32);
            let lane = (0..SCENE)
                .map(|n| {
                    let turn = (TAU * ECHO_HZ * n as f64 / RATE) as f32;
                    let echo = n.checked_sub(ECHO_DELAY).map_or(C32::default(), |at| {
                        reference[at] * C32::from_polar(0.05, turn)
                    });
                    let noise = C32::new((uniform() - 0.5) as f32, (uniform() - 0.5) as f32);
                    reference[n] * gain + echo + noise * 0.1
                })
                .collect();
            lanes.push(lane);
        }
        lanes
    })
}

fn block<'a>(lanes: &'a [&'a [C32]], first: u64, centers: &'a [f64]) -> ArrayBlock<'a> {
    ArrayBlock {
        lanes,
        corrected: true,
        correction: CorrectionView::identity(),
        first_index: first,
        unix_ns: BASE_NS + (first as f64 / RATE * 1e9) as u64,
        generation: 0,
        gap_before: false,
        centers_hz: centers,
        cal: CalView::default(),
        pose: Pose::default(),
    }
}

fn push_at(runner: &mut dyn DedicatedRunner, range: Range<usize>, center_hz: f64) {
    let lanes = scene();
    let centers = [center_hz; ELEMENTS];
    let views: [&[C32]; ELEMENTS] = std::array::from_fn(|element| &lanes[element][range.clone()]);
    runner.push(&block(&views, range.start as u64, &centers));
}

fn start(
    params: &PassiveRadarParams,
    backend: impl FnOnce(&StagePlan) -> Box<dyn CafBackend>,
) -> RadarWorker {
    let stage = stage_of(params, CENTER_HZ);
    let shared = Arc::new(Shared::default());
    RadarWorker::start(&stage, backend(&stage), HOST_BLOCK, shared).expect("worker")
}

fn cpu(stage: &StagePlan) -> Box<dyn CafBackend> {
    Box::new(CpuCaf::new(stage).expect("cpu"))
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

struct Collector {
    report: Option<Reading>,
    surface: Option<SurfaceFrame>,
    bearings: Vec<DfBearing>,
    lanes: Vec<LaneBuffer>,
    updates: Vec<RadarUpdate>,
    surfaces: Vec<RangeDopplerOwned>,
}

impl Collector {
    fn new() -> Self {
        Self {
            report: Some(Reading::new(
                PassiveRadarParams::TYPE_ID,
                RadarUpdate::reserved(),
            )),
            surface: Some(SurfaceFrame::RangeDoppler(RangeDopplerOwned::default())),
            bearings: Vec::new(),
            lanes: Vec::new(),
            updates: Vec::new(),
            surfaces: Vec::new(),
        }
    }

    fn poll(&mut self, runner: &mut dyn DedicatedRunner) -> bool {
        let tally = {
            let mut out = ProcessorOutput::new(OutputSlots {
                report: self.report.as_mut(),
                surface: self.surface.as_mut(),
                bearings: &mut self.bearings,
                lanes: &mut self.lanes,
            });
            runner.poll(&mut out);
            out.tally()
        };
        if tally.surface
            && let Some(SurfaceFrame::RangeDoppler(frame)) = &self.surface
        {
            self.surfaces.push(frame.clone());
        }
        if tally.report
            && let Some(update) = self.report.as_ref().and_then(Reading::get::<RadarUpdate>)
        {
            self.updates.push(update.clone());
        }
        tally.report
    }

    fn drain(&mut self, runner: &mut dyn DedicatedRunner) {
        while self.poll(runner) {}
    }

    fn tracks(&self) -> Vec<u32> {
        self.updates
            .last()
            .map_or_default(|update| update.tracks.iter().map(|track| track.id).collect())
    }

    fn lost(&self) -> Vec<u32> {
        self.updates
            .iter()
            .flat_map(|update| &update.events)
            .filter(|event| event.change == TrackChange::Lost)
            .map(|event| event.track_id)
            .collect()
    }
}

struct Rig {
    worker: RadarWorker,
    next: usize,
    center_hz: f64,
    seen: Collector,
}

impl Rig {
    fn new(params: &PassiveRadarParams) -> Self {
        Self::with(start(params, cpu))
    }

    fn with(worker: RadarWorker) -> Self {
        Self {
            worker,
            next: 0,
            center_hz: CENTER_HZ,
            seen: Collector::new(),
        }
    }

    fn push(&mut self, len: usize) {
        let end = (self.next + len).min(SCENE);
        push_at(&mut self.worker, self.next..end, self.center_hz);
        self.next = end;
    }

    /// Feeds one block and waits for the front stage alone, so a stalled CPI stage still sees
    /// every CPI while a slow host never drops blocks into gaps that restart the window.
    fn push_to_front(&mut self) {
        self.push(BLOCK);
        wait_for("the front stage", || self.worker.front_caught_up());
    }

    fn settle(&mut self) {
        let deadline = Instant::now() + PATIENCE;
        loop {
            self.seen.drain(&mut self.worker);
            if self.worker.settled() {
                self.seen.drain(&mut self.worker);
                return;
            }
            assert!(Instant::now() < deadline, "the radar did not settle");
            std::thread::sleep(Duration::from_micros(200));
        }
    }

    fn feed(&mut self, samples: usize) {
        let end = (self.next + samples).min(SCENE);
        while self.next < end {
            self.push(BLOCK.min(end - self.next));
            self.settle();
        }
    }

    fn feed_until(&mut self, what: &str, mut done: impl FnMut(&Collector) -> bool) {
        while !done(&self.seen) {
            assert!(self.next < SCENE, "the scene ran out waiting for {what}");
            self.push(BLOCK);
            self.settle();
        }
    }

    fn confirmed_track(&mut self) -> u32 {
        self.feed_until("a confirmed track", |seen| !seen.tracks().is_empty());
        self.seen.tracks()[0]
    }
}

struct Gated {
    inner: CpuCaf,
    gate: Arc<Mutex<()>>,
    entered: Arc<AtomicUsize>,
}

impl CafBackend for Gated {
    fn run(&mut self, job: &CpiJob, out: &mut CubeOut) -> Result<(), CafError> {
        self.entered.fetch_add(1, Ordering::AcqRel);
        drop(self.gate.lock().unwrap_or_else(PoisonError::into_inner));
        self.inner.run(job, out)
    }

    fn gpu(&self) -> bool {
        false
    }

    fn threads(&self) -> u32 {
        0
    }
}

struct Gate {
    gate: Arc<Mutex<()>>,
    entered: Arc<AtomicUsize>,
}

impl Gate {
    fn new() -> Self {
        Self {
            gate: Arc::new(Mutex::new(())),
            entered: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn backend(&self) -> impl FnOnce(&StagePlan) -> Box<dyn CafBackend> + use<> {
        let gate = Arc::clone(&self.gate);
        let entered = Arc::clone(&self.entered);
        move |stage| {
            Box::new(Gated {
                inner: CpuCaf::new(stage).expect("cpu"),
                gate,
                entered,
            })
        }
    }

    fn close(&self) -> MutexGuard<'_, ()> {
        self.gate.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn entered(&self) -> usize {
        self.entered.load(Ordering::Acquire)
    }
}

struct Slow {
    inner: CpuCaf,
    delay: Duration,
}

impl CafBackend for Slow {
    fn run(&mut self, job: &CpiJob, out: &mut CubeOut) -> Result<(), CafError> {
        std::thread::sleep(self.delay);
        self.inner.run(job, out)
    }

    fn gpu(&self) -> bool {
        false
    }

    fn threads(&self) -> u32 {
        0
    }
}

struct Explode;

impl CafBackend for Explode {
    fn run(&mut self, _job: &CpiJob, _out: &mut CubeOut) -> Result<(), CafError> {
        panic!("radar backend exploded on purpose");
    }

    fn gpu(&self) -> bool {
        false
    }

    fn threads(&self) -> u32 {
        0
    }
}

fn blocked_worker(gate: &Gate) -> RadarWorker {
    let mut worker = start(&params(), gate.backend());
    let mut next = 0;
    while gate.entered() == 0 {
        assert!(next < SCENE, "no CPI reached the backend");
        push_at(&mut worker, next..next + BLOCK, CENTER_HZ);
        next += BLOCK;
        wait_for("the front stage", || worker.front_caught_up());
    }
    worker
}

#[test]
fn push_never_allocates_or_blocks() {
    let mut worker = start(&params(), cpu);
    let lanes = scene();
    let centers = [CENTER_HZ; ELEMENTS];
    for at in (0..8 * BLOCK).step_by(BLOCK) {
        push_at(&mut worker, at..at + BLOCK, CENTER_HZ);
        std::thread::sleep(2 * PACE);
    }
    let dropped = worker.shared().dropped_blocks.load(Ordering::Acquire);
    let mut took = [Duration::ZERO; 32];
    assert_no_alloc("radar push", || {
        for (index, slot) in took.iter_mut().enumerate() {
            let at = (8 + index) * BLOCK;
            let views: [&[C32]; ELEMENTS] =
                std::array::from_fn(|element| &lanes[element][at..at + BLOCK]);
            let block = block(&views, at as u64, &centers);
            let started = Instant::now();
            worker.push(&block);
            *slot = started.elapsed();
            std::thread::sleep(2 * PACE);
        }
    });
    let copied =
        took.len() as u64 - (worker.shared().dropped_blocks.load(Ordering::Acquire) - dropped);
    assert!(copied >= 16, "only {copied} blocks reached the pool");
    took.sort_unstable();
    let block_period = Duration::from_secs_f64(BLOCK as f64 / RATE);
    assert!(took[took.len() / 2] < block_period / 100, "{took:?}");
    assert!(took[took.len() - 1] < Duration::from_millis(5), "{took:?}");
}

#[test]
fn a_block_with_the_wrong_lane_count_is_counted() {
    let mut worker = start(&params(), cpu);
    let lanes = scene();
    let views = [&lanes[0][..BLOCK], &lanes[1][..BLOCK]];
    let centers = [CENTER_HZ; 2];
    worker.push(&block(&views, 0, &centers));
    let faults = worker.faults();
    assert_eq!(faults.lane_mismatch, 1);
    assert_eq!(faults.dropped_blocks, 0);
    assert!(worker.running());
}

#[test]
fn a_full_input_pool_counts_dropped_samples_and_marks_a_gap() {
    let mut rig = Rig::new(&params());
    let shared = Arc::clone(rig.worker.shared());
    shared.hold_in.store(true, Ordering::Release);
    let mut pushed = 0;
    while shared.dropped_samples.load(Ordering::Acquire) == 0 {
        assert!(pushed < 64, "the input pool never filled");
        rig.push(BLOCK);
        pushed += 1;
    }
    assert_eq!(shared.dropped_samples.load(Ordering::Acquire), BLOCK as u64);
    assert_eq!(shared.dropped_blocks.load(Ordering::Acquire), 1);
    assert_eq!(shared.gaps_in.load(Ordering::Acquire), 0);
    shared.hold_in.store(false, Ordering::Release);
    rig.settle();
    rig.push(BLOCK);
    rig.settle();
    assert_eq!(shared.gaps_in.load(Ordering::Acquire), 1);
    rig.feed_until("a report after the gap", |seen| !seen.updates.is_empty());
    let health = &rig.seen.updates[0].health;
    assert_eq!(health.dropped_samples, BLOCK as u64);
    assert_eq!(health.dropped_cpis, 0);
}

#[test]
fn a_slow_cpi_stage_counts_dropped_cpis() {
    let mut worker = start(&params(), |stage| {
        Box::new(Slow {
            inner: CpuCaf::new(stage).expect("cpu"),
            delay: Duration::from_millis(300),
        })
    });
    let mut seen = Collector::new();
    for at in (0..SCENE / 2).step_by(BLOCK) {
        push_at(&mut worker, at..at + BLOCK, CENTER_HZ);
        seen.drain(&mut worker);
        std::thread::sleep(3 * PACE);
    }
    wait_for("a report counting dropped CPIs", || {
        seen.drain(&mut worker);
        seen.updates
            .iter()
            .any(|update| update.health.dropped_cpis > 0)
    });
    assert!(worker.shared().dropped_cpis.load(Ordering::Acquire) > 0);
}

#[test]
fn a_retune_discards_in_flight_cpis_and_ends_tracks() {
    let gate = Gate::new();
    let mut rig = Rig::with(start(&params(), gate.backend()));
    let track = rig.confirmed_track();
    let shared = Arc::clone(rig.worker.shared());
    let before = rig.seen.updates.len();
    let entered = gate.entered();
    let closed = gate.close();
    while shared.dropped_cpis.load(Ordering::Acquire) == 0 {
        assert!(rig.next < SCENE, "no CPI waited behind the stalled one");
        rig.push_to_front();
    }
    assert!(gate.entered() > entered);
    rig.center_hz = CENTER_HZ + 1e6;
    rig.push(BLOCK);
    drop(closed);
    wait_for("the retune to end the track", || {
        rig.seen.drain(&mut rig.worker);
        rig.seen.lost().contains(&track) && shared.discarded_cpis.load(Ordering::Acquire) > 0
    });
    rig.settle();
    rig.feed_until("a report after the retune", |seen| {
        seen.updates
            .last()
            .is_some_and(|update| update.health.discarded_cpis > 0)
    });
    let after_loss = rig
        .seen
        .updates
        .iter()
        .skip(before)
        .position(|update| {
            update
                .events
                .iter()
                .any(|event| event.track_id == track && event.change == TrackChange::Lost)
        })
        .expect("lost event");
    for update in rig.seen.updates.iter().skip(before + after_loss) {
        assert!(update.tracks.iter().all(|listed| listed.id != track));
    }
    assert!(shared.discarded_cpis.load(Ordering::Acquire) >= 1);
    assert_eq!(shared.retunes.load(Ordering::Acquire), 1);
}

#[test]
fn overlap_schedules_a_cpi_every_hop() {
    let overlapped = PassiveRadarParams {
        overlap: 0.5,
        ..params()
    };
    let stage = stage_of(&overlapped, CENTER_HZ);
    let mut rig = Rig::new(&overlapped);
    let fed = 1_000_000;
    rig.feed(fed);
    let window = stage.shape.window() as f64 * stage.front.input_rate / stage.front.radar_rate;
    let hop = stage.hop as f64 * stage.front.input_rate / stage.front.radar_rate;
    let expected = ((fed as f64 - window) / hop).floor() as usize + 1;
    let got = rig.seen.surfaces.len();
    assert!(
        got.abs_diff(expected) <= 1,
        "{got} CPIs, expected {expected}"
    );
    assert_eq!(rig.seen.updates.len(), got);
    let hop_ms = f64::from(rig.seen.updates[0].axes.hop_ms);
    assert!((hop_ms - stage.hop_s() * 1_000.0).abs() < 1e-3);
    for pair in rig.seen.surfaces.windows(2) {
        let step = pair[1].timestamp as f64 - pair[0].timestamp as f64;
        assert!((step - hop_ms).abs() <= 1.0, "{step} ms, hop {hop_ms} ms");
    }
    for (seq, update) in (0u64..).zip(&rig.seen.updates) {
        assert_eq!(update.seq, seq);
    }
}

#[test]
fn a_live_change_moves_the_hop_and_keeps_tracks() {
    let mut rig = Rig::new(&params());
    let track = rig.confirmed_track();
    let tuned = PassiveRadarParams {
        overlap: 0.5,
        cfar: CfarParams {
            min_snr_db: 9.0,
            ..CfarParams::default()
        },
        ..params()
    };
    let stage = stage_of(&tuned, CENTER_HZ);
    let live = stage.live();
    assert!(rig.worker.commit(Prepared::Live(Box::new(live))).is_none());
    let before = rig.seen.updates.len();
    rig.feed(600_000);
    let after = &rig.seen.updates[before..];
    assert!(after.len() >= 8, "{} CPIs after the change", after.len());
    let hop_ms = (stage.hop_s() * 1_000.0) as f32;
    let last = after.last().expect("update");
    assert!((last.axes.hop_ms - hop_ms).abs() < 1e-3);
    assert!(last.tracks.iter().any(|listed| listed.id == track));
    assert!(!rig.seen.lost().contains(&track));
}

#[test]
fn clearing_tracks_ends_them_in_the_next_report() {
    let mut rig = Rig::new(&params());
    let track = rig.confirmed_track();
    rig.worker
        .action(ProcessorAction::ClearTracks)
        .expect("clear");
    wait_for("the cleared track", || {
        rig.seen.drain(&mut rig.worker);
        rig.seen.lost().contains(&track)
    });
    let last = rig.seen.updates.last().expect("update");
    assert!(last.tracks.is_empty());
}

#[test]
fn a_rebuild_hands_its_tracks_and_ids_over() {
    let mut rig = Rig::new(&params());
    let track = rig.confirmed_track();
    let next = start(&params(), cpu);
    let old = rig
        .worker
        .commit(Prepared::Rebuild(Box::new(next)))
        .expect("the replaced runner");
    old.retire().join().expect("old radar stopped");
    wait_for("the old track to end", || {
        rig.seen.drain(&mut rig.worker);
        rig.seen.lost().contains(&track)
    });
    let fresh = rig.seen.updates.len();
    rig.feed_until("a track from the new runner", |seen| {
        seen.updates.len() > fresh && !seen.tracks().is_empty()
    });
    let new = rig.seen.tracks()[0];
    assert!(new > track, "{new} after {track}");
    assert!(rig.worker.running());
}

#[test]
fn drop_does_not_block_while_a_cpi_runs() {
    let gate = Gate::new();
    let closed = gate.close();
    let worker = blocked_worker(&gate);
    let shared = Arc::clone(worker.shared());
    let started = Instant::now();
    drop(worker);
    let took = started.elapsed();
    assert!(took < Duration::from_millis(5), "drop took {took:?}");
    assert!(shared.live.load(Ordering::Acquire) > 0);
    drop(closed);
    wait_for("the radar threads to exit", || {
        shared.live.load(Ordering::Acquire) == 0
    });
}

#[test]
fn retire_joins_every_thread() {
    let shared = Arc::new(Shared::default());
    let stage = stage_of(&params(), CENTER_HZ);
    let crew = crew::CrewCaf::new(&stage, 2, &shared).expect("crew");
    let worker = RadarWorker::start(&stage, Box::new(crew), HOST_BLOCK, Arc::clone(&shared))
        .expect("worker");
    assert_eq!(shared.live.load(Ordering::Acquire), 4);
    let mut rig = Rig::with(worker);
    rig.feed(400_000);
    assert!(!rig.seen.updates.is_empty());
    assert_eq!(rig.seen.updates[0].health.threads, 2);
    let runner: Box<dyn DedicatedRunner> = Box::new(rig.worker);
    runner.retire().join().expect("joined");
    assert_eq!(shared.live.load(Ordering::Acquire), 0);
    assert!(!shared.dead.load(Ordering::Acquire));
}

struct Failing;

impl CafBackend for Failing {
    fn run(&mut self, _job: &CpiJob, _out: &mut CubeOut) -> Result<(), CafError> {
        Err(CafError::Shape)
    }

    fn gpu(&self) -> bool {
        false
    }

    fn threads(&self) -> u32 {
        0
    }
}

#[test]
fn a_failed_cpi_is_reported_and_counted_as_dropped() {
    let mut rig = Rig::with(start(&params(), |_| Box::new(Failing)));
    rig.feed_until("a report of the failed CPI", |seen| seen.updates.len() >= 2);
    for (count, update) in (1u64..).zip(&rig.seen.updates) {
        assert_eq!(update.health.dropped_cpis, count);
        assert!(update.detections.is_empty() && update.tracks.is_empty());
    }
    assert!(rig.worker.running());
}

#[test]
fn settings_queued_behind_a_busy_cpi_are_kept_and_coalesced() {
    let gate = Gate::new();
    let mut rig = Rig::with(start(&params(), gate.backend()));
    let track = rig.confirmed_track();
    let entered = gate.entered();
    let closed = gate.close();
    while gate.entered() == entered {
        assert!(rig.next < SCENE, "no CPI reached the closed gate");
        rig.push_to_front();
    }
    rig.worker
        .action(ProcessorAction::ClearTracks)
        .expect("clear");
    let mut hop_ms = 0.0;
    for step in 1..=3 * COMMANDS {
        let tuned = PassiveRadarParams {
            overlap: 0.05 * step as f32 / (3 * COMMANDS) as f32 + 0.4,
            ..params()
        };
        let stage = stage_of(&tuned, CENTER_HZ);
        hop_ms = (stage.hop_s() * 1_000.0) as f32;
        assert!(
            rig.worker
                .commit(Prepared::Live(Box::new(stage.live())))
                .is_none()
        );
    }
    drop(closed);
    rig.feed_until("the last settings and the cleared track", |seen| {
        seen.lost().contains(&track)
            && seen
                .updates
                .last()
                .is_some_and(|update| (update.axes.hop_ms - hop_ms).abs() < 1e-3)
    });
}

#[test]
fn a_rebuild_carries_every_counter_over() {
    let gate = Gate::new();
    let mut rig = Rig::with(start(&params(), gate.backend()));
    rig.feed_until("a first report", |seen| !seen.updates.is_empty());
    let old_shared = Arc::clone(rig.worker.shared());
    let closed = gate.close();
    while old_shared.dropped_cpis.load(Ordering::Acquire) == 0 {
        assert!(
            rig.next < SCENE,
            "no CPI was dropped behind the closed gate"
        );
        rig.push_to_front();
    }
    let wrong = [&scene()[0][..BLOCK], &scene()[1][..BLOCK]];
    rig.worker.push(&block(&wrong, 0, &[CENTER_HZ; 2]));
    let old = rig
        .worker
        .commit(Prepared::Rebuild(Box::new(start(&params(), cpu))))
        .expect("the replaced runner");
    drop(closed);
    old.retire().join().expect("old radar stopped");
    let discarded = old_shared.discarded_cpis.load(Ordering::Acquire);
    let dropped = old_shared.dropped_cpis.load(Ordering::Acquire);
    assert!(discarded >= 1, "the waiting CPI was discarded on close");
    let fresh = rig.seen.updates.len();
    rig.feed_until("a report from the new runner", |seen| {
        seen.updates.len() > fresh + 1
    });
    let last = &rig.seen.updates.last().expect("update").health;
    assert!(last.discarded_cpis >= discarded, "{last:?}");
    assert!(last.dropped_cpis >= dropped, "{last:?}");
    for pair in rig.seen.updates.windows(2) {
        let (before, after) = (&pair[0].health, &pair[1].health);
        assert!(after.dropped_cpis >= before.dropped_cpis);
        assert!(after.discarded_cpis >= before.discarded_cpis);
        assert!(after.unsuppressed_groups >= before.unsuppressed_groups);
    }
    assert_eq!(rig.worker.faults().lane_mismatch, 1);
}

fn array_ctx<'a>(geometry: &'a ArrayGeometry, centers: &'a [f64]) -> ArrayCtx<'a> {
    ArrayCtx {
        node: "radar",
        lanes: ELEMENTS,
        sample_rate: RATE,
        center_hz: centers[0],
        lane_centers_hz: centers,
        geometry,
        positions_m: &[],
        manifold: None,
        tier: Coherence::PhaseCoherent,
        tuning: ArrayTuningMode::Together,
        max_block: HOST_BLOCK,
    }
}

#[test]
fn prepare_sorts_changes_into_same_live_and_rebuild() {
    let geometry = ArrayGeometry::Uca {
        radius_m: 0.35,
        first_deg: 0.0,
        winding: Winding::Clockwise,
    };
    let centers = [CENTER_HZ; ELEMENTS];
    let ctx = array_ctx(&geometry, &centers);
    let radar = |params: PassiveRadarParams| Params::new(params);
    let (runner, plan) = build(&ctx, &radar(params()), GpuUse::Off, &host()).expect("built");
    assert_ne!(plan.backend, CafBackendKind::Gpu);
    assert_eq!(&plan.stage.params, &params());
    let (same, kept) = prepare(&plan, &ctx, &radar(params()), GpuUse::Off, &host()).expect("same");
    assert!(matches!(same, Prepared::Same));
    assert_eq!(kept.backend, plan.backend);
    let stricter = PassiveRadarParams {
        cfar: CfarParams {
            min_snr_db: 12.0,
            ..CfarParams::default()
        },
        ..params()
    };
    let (live, tuned) = prepare(&plan, &ctx, &radar(stricter), GpuUse::Off, &host()).expect("live");
    match live {
        Prepared::Live(live) => {
            let live = live.downcast_ref::<LiveParams>().expect("radar settings");
            assert!((live.cfar.min_snr - 10f32.powf(1.2)).abs() < 1e-3);
        }
        _ => panic!("a CFAR change is live"),
    }
    assert_eq!(tuned.stage.params.cfar.min_snr_db, 12.0);
    let longer = PassiveRadarParams {
        cpi_ms: 300,
        ..params()
    };
    let (rebuilt, _) = prepare(&plan, &ctx, &radar(longer), GpuUse::Off, &host()).expect("rebuild");
    let Prepared::Rebuild(rebuilt) = rebuilt else {
        panic!("a CPI change rebuilds");
    };
    rebuilt.retire().join().expect("rebuilt radar stopped");
    let moved = [CENTER_HZ + 2e6; ELEMENTS];
    let retuned = array_ctx(&geometry, &moved);
    let (moved, moved_plan) =
        prepare(&plan, &retuned, &radar(params()), GpuUse::Off, &host()).expect("retune");
    let Prepared::Rebuild(moved) = moved else {
        panic!("a retune rebuilds");
    };
    assert!((moved_plan.stage.ctx.center_hz - (CENTER_HZ + 2e6)).abs() < 1e-6);
    moved.retire().join().expect("retuned radar stopped");
    let wrong = Params::new(crate::df::DfParams::default());
    assert!(matches!(
        prepare(&plan, &ctx, &wrong, GpuUse::Off, &host()),
        Err(ChannelError::Refused("Wrong settings"))
    ));
    Box::new(runner).retire().join().expect("radar stopped");
}

#[test]
fn gpu_off_keeps_the_caf_on_the_cpu() {
    let shared = Arc::new(Shared::default());
    let auto = PassiveRadarParams {
        gpu: GpuUse::Auto,
        ..params()
    };
    for (stage, engine) in [
        (stage_of(&auto, CENTER_HZ), GpuUse::Off),
        (stage_of(&params(), CENTER_HZ), GpuUse::Auto),
    ] {
        let (backend, kind) = select_backend(&stage, engine, &shared).expect("backend");
        assert_ne!(kind, CafBackendKind::Gpu);
        assert!(!backend.gpu());
    }
}

#[test]
fn a_dead_radar_thread_stops_the_runner() {
    let mut worker = start(&params(), |_| Box::new(Explode));
    let shared = Arc::clone(worker.shared());
    let mut next = 0;
    wait_for("the radar thread to die", || {
        if next + BLOCK <= SCENE {
            push_at(&mut worker, next..next + BLOCK, CENTER_HZ);
            next += BLOCK;
        }
        shared.dead.load(Ordering::Acquire)
    });
    assert!(!worker.running());
    let runner: Box<dyn DedicatedRunner> = Box::new(worker);
    assert!(runner.retire().join().is_err(), "the panic is surfaced");
}
