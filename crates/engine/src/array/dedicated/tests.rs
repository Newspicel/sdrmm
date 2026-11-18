use std::{
    any::Any,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use num_complex::Complex;
use sdrmm_array::{
    ArrayBlock, CalView, CorrectionView, Params, Pose, ProcessorAction, ProcessorFaults,
    ProcessorOutput, Retired,
    passive_radar::{PassiveRadarParams, ReferenceCleaning, RunnerPlan},
};
use sdrmm_wire::{ArrayOrientation, ArrayTuningMode, Coherence};

use super::*;
use crate::array::{
    Command, CommandQueue, LiveFrame,
    aggregator::{Aggregator, wire},
    board::StatusBoard,
    correct::CorrectionSet,
    host::{
        GateInputs, ProcessorHost,
        tests::{frame, plan, talk_params, talker_descriptor, taps},
    },
};

const LANES: usize = 3;
const RATE: f64 = 500_000.0;
const CENTER_HZ: f64 = 100e6;
const BLOCK: usize = 1_024;
const PATIENCE: Duration = Duration::from_secs(30);

/// A runner whose thread waits on a gate, and whose state the test sets.
struct Held {
    live: Arc<AtomicUsize>,
    running: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Held {
    fn new(gate: &Arc<Mutex<()>>) -> Self {
        let live = Arc::new(AtomicUsize::new(1));
        let gate = Arc::clone(gate);
        let left = Arc::clone(&live);
        let thread = thread::spawn(move || {
            drop(gate.lock().unwrap_or_else(PoisonError::into_inner));
            left.fetch_sub(1, Ordering::AcqRel);
        });
        Self {
            live,
            running: Arc::new(AtomicBool::new(true)),
            dropped: Arc::new(AtomicU64::new(0)),
            thread: Some(thread),
        }
    }

    fn open() -> Self {
        Self::new(&Arc::new(Mutex::new(())))
    }
}

impl DedicatedRunner for Held {
    fn push(&mut self, _block: &ArrayBlock<'_>) {}

    fn poll(&mut self, _out: &mut ProcessorOutput<'_>) {}

    fn commit(&mut self, prepared: Prepared) -> Option<Box<dyn DedicatedRunner>> {
        match prepared {
            Prepared::Rebuild(mut next) => {
                if let Some(held) = next.as_any_mut().downcast_mut::<Self>() {
                    std::mem::swap(self, held);
                }
                Some(next)
            }
            Prepared::Same | Prepared::Live(_) => None,
        }
    }

    fn action(&mut self, _action: ProcessorAction) -> Result<(), ChannelError> {
        Ok(())
    }

    fn faults(&self) -> ProcessorFaults {
        ProcessorFaults::default()
    }

    fn dropped_samples(&self) -> u64 {
        self.dropped.load(Ordering::Acquire)
    }

    fn running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn retire(mut self: Box<Self>) -> Retired {
        Retired::new(self.thread.take().into_iter().collect())
    }
}

fn live_frame() -> LiveFrame {
    LiveFrame {
        sample_rate: RATE,
        center_hz: CENTER_HZ,
        lane_centers_hz: vec![CENTER_HZ; LANES],
        orientation: ArrayOrientation::Fixed { azimuth_deg: 0.0 },
        tier: Coherence::PhaseCoherent,
        keeps_phase: true,
        needs_time: true,
        tuning: ArrayTuningMode::Together,
        dc_block: false,
        in_flight: 0,
        devices: [0; 16],
    }
}

fn hosted(runner: Box<dyn DedicatedRunner>, sinks: &crate::array::HostSinks) -> Box<ProcessorHost> {
    ProcessorHost::dedicated(
        plan("held", talk_params(), sinks, LANES),
        &live_frame(),
        talker_descriptor(),
        runner,
    )
    .expect("a dedicated host")
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < PATIENCE, "waited too long for {what}");
        thread::sleep(Duration::from_millis(1));
    }
}

const OPEN: GateInputs = GateInputs {
    sample_rate: RATE,
    window: None,
    tier: Coherence::PhaseCoherent,
    tuning: ArrayTuningMode::Together,
    synced: true,
    phase_ready: true,
    gain_ready: true,
};

fn drive(host: &mut ProcessorHost) {
    let identity = CorrectionSet::identity(LANES);
    let samples = [Complex::new(0.1f32, 0.0); BLOCK];
    let lanes: [&[Complex<f32>]; LANES] = [&samples; LANES];
    let centers = [CENTER_HZ; LANES];
    host.process(
        &ArrayBlock {
            lanes: &lanes,
            corrected: false,
            correction: CorrectionView::identity(),
            first_index: 0,
            unix_ns: 0,
            generation: 0,
            gap_before: false,
            centers_hz: &centers,
            cal: CalView::default(),
            pose: Pose::default(),
        },
        &OPEN,
        &identity,
    );
    host.poll(CENTER_HZ);
}

#[test]
fn a_processor_without_a_runner_is_refused() {
    let taps = taps(0);
    let plan = plan("talker", talk_params(), &taps.sinks, 2);
    let frame = frame(2);
    let ctx = plan.shape.ctx(&plan.node, &frame);
    let Err(ChannelError::Unsupported(text)) =
        build_dedicated(talker_descriptor().as_ref(), &ctx, &plan.params)
    else {
        panic!("a processor with no runner is refused");
    };
    assert_eq!(text, "Talker has no runner");
}

#[test]
fn a_replaced_runner_retires_off_the_aggregator() {
    let (queue, commands) = CommandQueue::new();
    let wiring = wire(LANES, RATE, Arc::new(AtomicBool::new(false))).expect("wire");
    let mut aggregator = Aggregator::new(
        (0..LANES).map(|_| None).collect(),
        Box::new(live_frame()),
        Arc::new(StatusBoard::new(LANES)),
        commands,
        wiring.aggregator,
        None,
        None,
    );
    let taps = taps(0);
    let mut send = |command: Command| {
        queue.send(command).expect("queued");
        let started = Instant::now();
        aggregator.step();
        started.elapsed()
    };
    let gate = Arc::new(Mutex::new(()));
    let closed = gate.lock().unwrap_or_else(PoisonError::into_inner);
    let old = Held::new(&gate);
    let old_live = Arc::clone(&old.live);
    send(Command::ReplaceHost {
        host: hosted(Box::new(old), &taps.sinks),
    });
    let took = send(Command::ReplaceHost {
        host: hosted(Box::new(Held::open()), &taps.sinks),
    });
    assert!(took < Duration::from_millis(50), "replace took {took:?}");
    assert_eq!(
        old_live.load(Ordering::Acquire),
        1,
        "still held by the gate"
    );
    drop(closed);
    wait_for("the replaced runner to retire", || {
        old_live.load(Ordering::Acquire) == 0
    });
}

#[test]
fn a_stopped_runner_is_reported() {
    let held = Held::open();
    held.running.store(false, Ordering::Release);
    let taps = taps(0);
    let mut host = hosted(Box::new(held), &taps.sinks);
    drive(&mut host);
    let status = host.stats().status("held", "talker", None);
    assert!(!status.running);
    assert_eq!(status.error.as_deref(), Some("Stopped"));
    host.retire();
}

#[test]
fn a_dedicated_host_status_counts_the_dropped_samples() {
    let held = Held::open();
    held.dropped.store(4_096, Ordering::Release);
    let taps = taps(0);
    let mut host = hosted(Box::new(held), &taps.sinks);
    drive(&mut host);
    let status = host.stats().status("held", "talker", None);
    assert_eq!(status.dropped_samples, 4_096);
    host.retire();
}

#[test]
fn a_committed_rebuild_reopens_a_rebuilding_host() {
    let taps = taps(0);
    let mut host = hosted(Box::new(Held::open()), &taps.sinks);
    host.stats().rebuild.store(true, Ordering::Relaxed);
    let old = host
        .commit(Prepared::Rebuild(Box::new(Held::open())))
        .expect("the replaced runner");
    old.retire().join().expect("old runner stopped");
    assert!(!host.stats().wants_rebuild());
    assert!(host.commit(Prepared::Same).is_none());
    host.poll(CENTER_HZ);
    let status = host.stats().status("held", "talker", None);
    assert!(status.running);
    assert_eq!(status.error, None);
    host.retire();
}

#[test]
fn a_passive_radar_host_builds_its_runner_on_the_engine() {
    let taps = taps(0);
    let params = Params::new(PassiveRadarParams {
        cpi_ms: 200,
        aoa: false,
        reference: ReferenceCleaning::Off,
        ..PassiveRadarParams::default()
    });
    let built = ProcessorHost::build(plan("radar", params, &taps.sinks, LANES), &live_frame())
        .expect("radar host");
    let plan = built.dedicated.expect("radar plan");
    let plan = plan
        .as_any()
        .downcast_ref::<RunnerPlan>()
        .expect("a radar plan");
    assert_eq!(plan.stage.ctx.elements, LANES);
    assert!((plan.stage.front.radar_rate - RATE / 2.0).abs() < 1e-6);
    built.host.retire();
}
