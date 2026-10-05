use std::{
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

use sdrmm_wire::{ScanMode, ScanState};
use tokio::sync::broadcast::{Receiver, error::TryRecvError};

use super::{
    Call, HOLD_POLL, Halt, POLL, PRIORITY_INTERVAL, SPECTRUM_TIMEOUT, Scan, lock_status,
    measure_mean,
};
use crate::{Engine, runtime::SpectrumSnapshot};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stay {
    UntilQuiet,
    For(Duration),
}

impl Scan {
    pub(super) fn take(
        &mut self,
        engine: &Arc<Engine>,
        rx: &mut Receiver<SpectrumSnapshot>,
        call: Call,
    ) -> Result<(), Halt> {
        self.note_hit(call);
        if self.settings.mode == ScanMode::All {
            self.visited.push(call.hz);
            self.found_new = true;
            self.last_visit = Some(call.hz);
            let visit = Duration::from_millis(u64::from(self.settings.hold_ms));
            return self.hold(engine, rx, call, Stay::For(visit));
        }
        self.hold(engine, rx, call, Stay::UntilQuiet)
    }

    pub(super) fn hold(
        &mut self,
        engine: &Arc<Engine>,
        rx: &mut Receiver<SpectrumSnapshot>,
        call: Call,
        stay: Stay,
    ) -> Result<(), Halt> {
        let mut held = (call, stay);
        while let Some(preempt) = self.hold_once(engine, rx, held.0, held.1)? {
            self.note_hit(preempt);
            held = (preempt, Stay::UntilQuiet);
        }
        Ok(())
    }

    fn hold_once(
        &mut self,
        engine: &Arc<Engine>,
        rx: &mut Receiver<SpectrumSnapshot>,
        call: Call,
        stay: Stay,
    ) -> Result<Option<Call>, Halt> {
        self.release.store(false, Ordering::Release);
        {
            let mut status = lock_status(&self.status);
            status.state = ScanState::Holding;
            status.current_hz = call.hz;
        }
        self.follow(engine, true)?;
        self.push_update(engine, true);

        let probes = self.hold_probes(call.hz, stay);
        let mut snrs = vec![f32::NEG_INFINITY; probes.len()];
        let started = Instant::now();
        let resume = Duration::from_millis(u64::from(self.settings.resume_ms));
        let mut quiet_since: Option<Instant> = None;
        let mut preempt = None;
        loop {
            self.check_stop()?;
            if self.release.swap(false, Ordering::AcqRel) {
                break;
            }
            self.listen(rx, &probes, &mut snrs, HOLD_POLL)?;
            lock_status(&self.status).current_snr_db = snrs[0].is_finite().then_some(snrs[0]);
            self.push_update(engine, false);
            preempt = self.busy_calls(&probes[1..], &snrs[1..]).next();
            if preempt.is_some() {
                break;
            }
            let over = match stay {
                Stay::For(visit) => started.elapsed() >= visit,
                Stay::UntilQuiet if snrs[0] >= self.settings.margin_db => {
                    quiet_since = None;
                    false
                }
                Stay::UntilQuiet => {
                    quiet_since.get_or_insert_with(Instant::now).elapsed() >= resume
                }
            };
            if over {
                break;
            }
        }
        lock_status(&self.status).state = ScanState::Scanning;
        self.push_update(engine, true);
        Ok(preempt)
    }

    fn hold_probes(&self, hz: f64, stay: Stay) -> Vec<f64> {
        let mut probes = vec![hz];
        if stay == Stay::UntilQuiet {
            probes.extend(
                self.plan
                    .priority
                    .iter()
                    .filter(|&&priority| (priority - hz).abs() >= self.bw_hz),
            );
        }
        probes
    }

    pub(super) fn priority_due(&self) -> bool {
        self.settings.mode != ScanMode::All
            && !self.plan.priority.is_empty()
            && self
                .last_priority
                .is_none_or(|at| at.elapsed() >= PRIORITY_INTERVAL)
    }

    pub(super) fn check_priority(
        &mut self,
        engine: &Arc<Engine>,
        rx: &mut Receiver<SpectrumSnapshot>,
    ) -> Result<(), Halt> {
        let dwell = self.dwell();
        for index in 0..self.plan.priority.len() {
            let hz = self.plan.priority[index];
            if self.park(engine, rx, hz)?.is_none() {
                continue;
            }
            let mut snr = [f32::NEG_INFINITY];
            self.listen(rx, &[hz], &mut snr, dwell)?;
            let busy = self.busy_calls(&[hz], &snr).next();
            if let Some(call) = busy {
                self.take(engine, rx, call)?;
            }
        }
        self.last_priority = Some(Instant::now());
        Ok(())
    }

    pub(super) fn listen(
        &mut self,
        rx: &mut Receiver<SpectrumSnapshot>,
        targets: &[f64],
        snrs: &mut [f32],
        window: Duration,
    ) -> Result<(), Halt> {
        snrs.fill(f32::NEG_INFINITY);
        let start = Instant::now();
        let deadline = start + window;
        let mut frames = 0usize;
        loop {
            self.check_stop()?;
            let now = Instant::now();
            if frames > 0 {
                if now >= deadline {
                    return Ok(());
                }
            } else if now.duration_since(start) >= SPECTRUM_TIMEOUT {
                return Err(Halt::Failed(format!(
                    "the device produced no spectrum within {SPECTRUM_TIMEOUT:?}"
                )));
            }
            match rx.try_recv() {
                Ok(snapshot) => {
                    frames += 1;
                    self.measure_into(&snapshot, targets, snrs);
                }
                Err(TryRecvError::Empty) => std::thread::sleep(POLL),
                Err(TryRecvError::Lagged(_)) => {}
                Err(TryRecvError::Closed) => return Err(Halt::Stopped),
            }
        }
    }

    fn measure_into(&mut self, snapshot: &SpectrumSnapshot, targets: &[f64], snrs: &mut [f32]) {
        let Some(floor) = self.floor.of(snapshot) else {
            return;
        };
        for (snr, &target) in snrs.iter_mut().zip(targets) {
            if let Some(db) = measure_mean(snapshot, target, self.bw_hz) {
                *snr = snr.max(db - floor);
            }
        }
    }
}
