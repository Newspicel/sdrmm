use std::{
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

use sdrmm_wire::{HeardSignal, ScanMode, ScanState};
use tokio::sync::broadcast::{Receiver, error::TryRecvError};

use super::{
    Call, Halt, JUMP_LINGER, POLL, SPECTRUM_TIMEOUT, Scan, drain, local_peaks, lock_status,
    measure_mean, millis,
};
use crate::{ChannelInfo, DeviceSetState, Engine, hunt::unix_ms, runtime::SpectrumSnapshot};

/// How long a held frequency has to stay quiet before a busy one elsewhere takes the decoder, so
/// a fade does not throw away a weak call.
const QUIET_GAP: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Pick {
    Listed,
    Loudest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Move {
    Stay,
    Hold(usize),
    Release,
}

/// Every target of a scan whose whole plan sits in one window, read from each frame at once.
pub(super) struct Board {
    targets: Vec<f64>,
    priority: Vec<bool>,
    bw_hz: f64,
    margin_db: f32,
    resume: Duration,
    pick: Pick,
    busy: Vec<bool>,
    was_busy: Vec<bool>,
    fresh: Vec<usize>,
    held: Option<usize>,
    /// The hold is the operator's jump, not a call.
    pinned: bool,
    parked: usize,
    quiet_since: Option<Instant>,
}

impl Board {
    pub(super) fn new(
        targets: Vec<f64>,
        priority: &[f64],
        bw_hz: f64,
        (margin_db, resume): (f32, Duration),
        pick: Pick,
    ) -> Self {
        let n = targets.len();
        let priority = targets.iter().map(|hz| priority.contains(hz)).collect();
        Self {
            parked: middle(&targets),
            targets,
            priority,
            bw_hz,
            margin_db,
            resume,
            pick,
            busy: vec![false; n],
            was_busy: vec![false; n],
            fresh: Vec::new(),
            held: None,
            pinned: false,
            quiet_since: None,
        }
    }

    pub(super) fn parked(&self) -> usize {
        self.parked
    }

    /// Targets that went busy on the last frame.
    pub(super) fn fresh(&self) -> &[usize] {
        &self.fresh
    }

    pub(super) fn drop_hold(&mut self) {
        self.held = None;
        self.pinned = false;
        self.quiet_since = None;
    }

    /// The target nearest `hz`.
    pub(super) fn nearest(&self, hz: f64) -> usize {
        (0..self.targets.len())
            .min_by(|&a, &b| {
                (self.targets[a] - hz)
                    .abs()
                    .total_cmp(&(self.targets[b] - hz).abs())
            })
            .unwrap_or(0)
    }

    /// Holds target `i` for the operator until it has been quiet for the jump linger.
    pub(super) fn pin(&mut self, i: usize) {
        self.take(i);
        self.pinned = true;
    }

    pub(super) fn frame(&mut self, snrs: &[f32], locked: &[bool], now: Instant) -> Move {
        self.busy.fill(false);
        for i in local_peaks(&self.targets, snrs, self.margin_db, self.bw_hz) {
            self.busy[i] = !locked[i];
        }
        self.fresh.clear();
        self.fresh
            .extend((0..self.busy.len()).filter(|&i| self.busy[i] && !self.was_busy[i]));
        self.was_busy.copy_from_slice(&self.busy);
        self.decide(snrs, locked, now)
    }

    fn decide(&mut self, snrs: &[f32], locked: &[bool], now: Instant) -> Move {
        let held_on = self
            .held
            .is_some_and(|i| snrs[i] >= self.margin_db && !locked[i]);
        if self.pinned {
            return self.linger(held_on, now);
        }
        if let Some(p) = self.busy_priority()
            && self.held != Some(p)
            && self.held.is_none_or(|h| !held_on || !self.priority[h])
        {
            return self.take(p);
        }
        if held_on {
            self.quiet_since = None;
            return Move::Stay;
        }
        let quiet = self
            .held
            .map(|_| now.duration_since(*self.quiet_since.get_or_insert(now)));
        if quiet.is_none_or(|q| q >= QUIET_GAP)
            && let Some(next) = self.candidate(snrs)
        {
            return self.take(next);
        }
        if quiet.is_some_and(|q| q >= self.resume) {
            self.drop_hold();
            return Move::Release;
        }
        Move::Stay
    }

    fn linger(&mut self, held_on: bool, now: Instant) -> Move {
        if held_on {
            self.quiet_since = None;
            return Move::Stay;
        }
        let quiet = now.duration_since(*self.quiet_since.get_or_insert(now));
        if quiet >= self.resume.max(JUMP_LINGER) {
            self.drop_hold();
            return Move::Release;
        }
        Move::Stay
    }

    fn take(&mut self, i: usize) -> Move {
        self.held = Some(i);
        self.pinned = false;
        self.parked = i;
        self.quiet_since = None;
        Move::Hold(i)
    }

    fn busy_priority(&self) -> Option<usize> {
        (0..self.busy.len()).find(|&i| self.busy[i] && self.priority[i])
    }

    fn candidate(&self, snrs: &[f32]) -> Option<usize> {
        let spill = |i: usize| {
            self.held
                .is_some_and(|h| (self.targets[i] - self.targets[h]).abs() < self.bw_hz)
        };
        let mut open = (0..self.busy.len()).filter(|&i| self.busy[i] && !spill(i));
        match self.pick {
            Pick::Listed => open.next(),
            Pick::Loudest => open.max_by(|&a, &b| snrs[a].total_cmp(&snrs[b])),
        }
    }
}

pub(super) fn middle(targets: &[f64]) -> usize {
    let (Some(first), Some(last)) = (targets.first(), targets.last()) else {
        return 0;
    };
    let mid = (first + last) / 2.0;
    let after = targets
        .partition_point(|&hz| hz < mid)
        .min(targets.len() - 1);
    let before = after.saturating_sub(1);
    if (targets[before] - mid).abs() <= (targets[after] - mid).abs() {
        before
    } else {
        after
    }
}

/// Whether every target's band sits inside the frame.
pub(super) fn fits(targets: &[f64], snapshot: &SpectrumSnapshot, bw_hz: f64) -> bool {
    let half = f64::from(snapshot.span_hz) / 2.0 - bw_hz / 2.0;
    match (targets.first(), targets.last()) {
        (Some(&first), Some(&last)) => {
            half > 0.0 && first >= snapshot.center_hz - half && last <= snapshot.center_hz + half
        }
        _ => false,
    }
}

fn frames_whole((first, last): (f64, f64), band_hz: f64, rate: f64) -> bool {
    last - first + band_hz <= rate
}

impl DeviceSetState {
    /// Copies of each scanning decoder at both ends of what it reads at once: its whole plan
    /// when that fits one window, so the scan can watch every target together, or else the
    /// window it is stepping through. Auto tuning frames them like any decoder.
    pub(crate) fn scan_edges(&self) -> Vec<ChannelInfo> {
        let rate = crate::sample_rate_of(&self.settings);
        let mut edges = Vec::new();
        for (id, scanner) in &self.scanners {
            let Some(channel) = self.channels.iter().find(|c| c.id == *id) else {
                continue;
            };
            let (low, high) = sdrmm_decoder::occupied_band(&channel.settings.params);
            let span = if frames_whole(scanner.edges, high - low, rate) {
                Some(scanner.edges)
            } else {
                scanner.window.edges()
            };
            let Some((first, last)) = span else {
                continue;
            };
            for hz in [first, last] {
                let mut edge = channel.clone();
                edge.settings.frequency_hz = hz;
                edges.push(edge);
            }
        }
        edges
    }
}

impl Scan {
    fn plan_could_fit(&self, rate: f64) -> bool {
        match (self.plan.targets.first(), self.plan.targets.last()) {
            (Some(&first), Some(&last)) => frames_whole((first, last), self.bw_hz, rate),
            _ => false,
        }
    }

    fn framed(
        &mut self,
        engine: &Arc<Engine>,
        rx: &mut Receiver<SpectrumSnapshot>,
    ) -> Result<bool, Halt> {
        let centre = self.plan.targets[middle(&self.plan.targets)];
        let Some(window) = self.park(engine, rx, centre)? else {
            return Ok(false);
        };
        if fits(&self.plan.targets, &window, self.bw_hz) {
            return Ok(true);
        }
        if !engine.settle_tuning(self.ds) {
            return Ok(false);
        }
        self.settle.moved();
        drain(rx);
        Ok(self
            .fresh_frame(rx, centre)?
            .is_some_and(|window| fits(&self.plan.targets, &window, self.bw_hz)))
    }

    /// Watches the whole plan at once while it fits the radio's window. Returns once it no
    /// longer does, or straight away when it never did.
    pub(super) fn watch_live(&mut self, engine: &Arc<Engine>, rate: f64) -> Result<(), Halt> {
        if self.settings.mode == ScanMode::All || self.in_sweep || !self.plan_could_fit(rate) {
            return Ok(());
        }
        let mut rx = engine
            .subscribe_spectrum(self.ds, self.stream)
            .map_err(|e| Halt::Failed(e.to_string()))?;
        if !self.framed(engine, &mut rx)? {
            return Ok(());
        }
        let pick = match self.settings.mode {
            ScanMode::CloseCall => Pick::Loudest,
            ScanMode::Targets | ScanMode::All => Pick::Listed,
        };
        let mut board = Board::new(
            self.plan.targets.clone(),
            &self.plan.priority,
            self.bw_hz,
            (
                self.settings.margin_db,
                Duration::from_millis(u64::from(self.settings.resume_ms)),
            ),
            pick,
        );
        self.set_live(engine, true);
        let outcome = self.live_frames(engine, &mut rx, &mut board);
        self.set_live(engine, false);
        outcome
    }

    fn set_live(&mut self, engine: &Engine, live: bool) {
        lock_status(&self.status).live = live;
        self.push_update(engine, true);
    }

    fn live_frames(
        &mut self,
        engine: &Arc<Engine>,
        rx: &mut Receiver<SpectrumSnapshot>,
        board: &mut Board,
    ) -> Result<(), Halt> {
        let n = self.plan.targets.len();
        let mut snrs = vec![f32::NEG_INFINITY; n];
        let mut locked = vec![false; n];
        let mut last_frame: Option<Instant> = None;
        loop {
            let snapshot = self.next_frame(rx)?;
            let now = Instant::now();
            if let Some(before) = last_frame.replace(now) {
                lock_status(&self.status).round_ms = Some(millis(now.duration_since(before)));
            }
            if !fits(&self.plan.targets, &snapshot, self.bw_hz) {
                return Ok(());
            }
            let Some(floor) = self.floor.of(&snapshot) else {
                continue;
            };
            for (snr, &hz) in snrs.iter_mut().zip(&self.plan.targets) {
                *snr = measure_mean(&snapshot, hz, self.bw_hz)
                    .map_or(f32::NEG_INFINITY, |db| db - floor);
            }
            for (lock, &hz) in locked.iter_mut().zip(&self.plan.targets) {
                *lock = self.locked_out(hz);
            }
            if self.release.swap(false, Ordering::AcqRel) {
                board.drop_hold();
                lock_status(&self.status).state = ScanState::Scanning;
            }
            if let Some(hz) = self.take_jump() {
                let i = board.nearest(hz);
                board.pin(i);
                self.hold_live(engine, rx, self.plan.targets[i])?;
            }
            let step = board.frame(&snrs, &locked, Instant::now());
            self.note_fresh(board.fresh(), &snrs);
            self.live_step(engine, rx, step, &snrs)?;
            {
                let mut status = lock_status(&self.status);
                let snr = snrs[board.parked()];
                status.current_snr_db = snr.is_finite().then_some(snr);
                status.sweeps += 1;
            }
            self.push_update(engine, false);
        }
    }

    fn note_fresh(&self, fresh: &[usize], snrs: &[f32]) {
        if fresh.is_empty() {
            return;
        }
        let at_ms = unix_ms();
        let mut status = lock_status(&self.status);
        for &i in fresh {
            status.note_heard(HeardSignal {
                hz: self.plan.targets[i],
                snr_db: snrs[i],
                at_ms,
            });
        }
    }

    fn live_step(
        &mut self,
        engine: &Arc<Engine>,
        rx: &mut Receiver<SpectrumSnapshot>,
        step: Move,
        snrs: &[f32],
    ) -> Result<(), Halt> {
        match step {
            Move::Stay => Ok(()),
            Move::Release => {
                lock_status(&self.status).state = ScanState::Scanning;
                self.push_update(engine, true);
                Ok(())
            }
            Move::Hold(i) => {
                let hz = self.plan.targets[i];
                self.note_hit(Call {
                    hz,
                    snr_db: snrs[i],
                });
                self.hold_live(engine, rx, hz)
            }
        }
    }

    fn hold_live(
        &mut self,
        engine: &Arc<Engine>,
        rx: &mut Receiver<SpectrumSnapshot>,
        hz: f64,
    ) -> Result<(), Halt> {
        {
            let mut status = lock_status(&self.status);
            status.state = ScanState::Holding;
            status.current_hz = hz;
        }
        self.steer(engine, rx, hz)?;
        self.push_update(engine, true);
        Ok(())
    }

    fn next_frame(&self, rx: &mut Receiver<SpectrumSnapshot>) -> Result<SpectrumSnapshot, Halt> {
        let start = Instant::now();
        loop {
            self.check_stop()?;
            match rx.try_recv() {
                Ok(snapshot) => return Ok(snapshot),
                Err(TryRecvError::Empty) => {
                    if start.elapsed() >= SPECTRUM_TIMEOUT {
                        return Err(Halt::Failed(format!(
                            "the device produced no spectrum within {SPECTRUM_TIMEOUT:?}"
                        )));
                    }
                    std::thread::sleep(POLL);
                }
                Err(TryRecvError::Lagged(_)) => {}
                Err(TryRecvError::Closed) => return Err(Halt::Stopped),
            }
        }
    }
}

#[cfg(test)]
mod tests;
