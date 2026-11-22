use num_complex::Complex;

use super::{
    acquire::Acquire,
    decode::Decoder,
    frame::Frame,
    sis::Station,
    sync::{State, Sync},
};

/// PIDS frames in a row that may fail their CRC before the lock counts as false: a block
/// sync on a sample slip or a wrong control word reads nothing at all.
const LOST_PIDS: u8 = 8;

/// What one block brought.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Block {
    /// A PIDS frame was decoded; whether it passed its CRC.
    pub pids: Option<bool>,
    /// A P1 frame was decoded; whether its control word was valid.
    pub p1: Option<bool>,
}

/// The hybrid FM receiver from input samples to station information and HDC packets.
pub struct Receiver {
    acquire: Acquire,
    sync: Sync,
    decoder: Decoder,
    pub frame: Frame,
    pub station: Station,
    pub p1_ok: u32,
    pub p1_bad: u32,
    pids_failures: u8,
}

impl Receiver {
    pub fn new() -> Self {
        Self {
            acquire: Acquire::new(),
            sync: Sync::new(),
            decoder: Decoder::new(),
            frame: Frame::new(),
            station: Station::default(),
            p1_ok: 0,
            p1_bad: 0,
            pids_failures: 0,
        }
    }

    pub fn reset(&mut self) {
        self.acquire.reset();
        self.sync.reset();
        self.decoder.reset();
        self.frame.reset();
        self.station = Station::default();
        self.p1_ok = 0;
        self.p1_bad = 0;
        self.pids_failures = 0;
    }

    pub fn locked(&self) -> bool {
        self.sync.state == State::Locked
    }

    pub fn mer_db(&self) -> f32 {
        self.sync.mer_db
    }

    pub fn offset_hz(&self) -> f64 {
        self.acquire.offset_hz()
    }

    pub fn bit_error_rate(&self) -> f32 {
        self.decoder.p1_errors
    }

    pub fn service_mode(&self) -> u8 {
        self.sync.psmi
    }

    /// The last block's first `symbols` symbols of equalised QPSK data subcarriers.
    pub fn points(&self, symbols: usize) -> impl Iterator<Item = Complex<f32>> + '_ {
        self.sync.points(symbols)
    }

    pub fn data_carriers(&self) -> usize {
        self.sync.data_carriers()
    }

    /// Takes samples up to the end of the next block; returns how many it took and, when a
    /// block was complete, what it brought.
    pub fn push(&mut self, iq: &[Complex<f32>]) -> (usize, Option<Block>) {
        let taken = self.acquire.push(iq);
        if !self.acquire.full() {
            return (taken, None);
        }
        (taken, Some(self.block()))
    }

    fn block(&mut self) -> Block {
        let tracking = self.locked().then_some(self.sync.tracking).flatten();
        let step = self.acquire.process(tracking, &mut self.sync.bins);
        self.sync.retime(step);
        let was_locked = self.locked();
        if !was_locked {
            self.sync.state = State::Coarse;
        }
        let outcome = self.sync.process();
        if let Some((bins, symbols)) = outcome.shift {
            self.acquire.shift_carriers(bins);
            self.acquire.delay(symbols);
        }
        if !was_locked && self.locked() {
            self.decoder.reset();
            self.frame.reset();
        }
        let mut block = Block::default();
        if let Some(index) = outcome.block {
            let p1 = self.decoder.push(index, &self.sync.soft);
            let pids_ok = self.station.push(&self.decoder.pids);
            block.pids = Some(pids_ok);
            self.pids_failures = if pids_ok {
                0
            } else {
                self.pids_failures.saturating_add(1)
            };
            if self.pids_failures >= LOST_PIDS {
                self.pids_failures = 0;
                self.sync.state = State::Searching;
                return block;
            }
            if p1 {
                let ok = self.frame.push(&self.decoder.p1);
                block.p1 = Some(ok);
                if ok {
                    self.p1_ok = self.p1_ok.saturating_add(1);
                } else {
                    self.p1_bad = self.p1_bad.saturating_add(1);
                    self.sync.state = State::Searching;
                }
            }
        }
        block
    }
}
