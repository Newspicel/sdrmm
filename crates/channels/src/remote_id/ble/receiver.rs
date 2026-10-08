use num_complex::Complex;
use sdrmm_dsp::FmDemod;
use sdrmm_wire::RemoteIdPhy;

use super::{
    coded::{self, SyncCoded},
    gfsk::{self, DEVIATION_HZ, Level, RATE_HZ, Read, Reader, SPS, Sync1m},
};

const LOOKAHEAD: usize = coded::SYNC_SAMPLES + 64;
const REFINE: usize = 1;
const MAX_HISTORY: usize = 80_000;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Packet {
    pub pdu: Vec<u8>,
    pub phy: RemoteIdPhy,
    pub rf: Option<u8>,
    pub level_dbfs: f32,
    pub sample: u64,
}

pub(crate) struct Syncs {
    one: Sync1m,
    coded: SyncCoded,
}

impl Syncs {
    pub(crate) fn new() -> Self {
        Self {
            one: Sync1m::new(),
            coded: SyncCoded::new(),
        }
    }
}

enum Found {
    Packet(Packet, usize),
    Wait,
    Nothing,
}

pub(crate) struct Lane {
    rf: Option<u8>,
    channel_index: u8,
    demod: FmDemod,
    frequency: Vec<f32>,
    history: Vec<f32>,
    rising: Vec<u64>,
    power: Vec<f32>,
    tail: [f32; SPS],
    next: usize,
    running: f32,
    consumed: u64,
    cursor: usize,
}

impl Lane {
    pub(crate) fn new(rf: Option<u8>, channel_index: u8) -> Self {
        Self {
            rf,
            channel_index,
            demod: FmDemod::new(RATE_HZ, DEVIATION_HZ),
            frequency: Vec::new(),
            history: Vec::new(),
            rising: Vec::new(),
            power: Vec::new(),
            tail: [0.0; SPS],
            next: 0,
            running: 0.0,
            consumed: 0,
            cursor: 0,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.history.clear();
        self.rising.clear();
        self.power.clear();
        self.consumed = 0;
        self.cursor = 0;
        self.tail = [0.0; SPS];
        self.running = 0.0;
    }

    pub(crate) fn process(&mut self, iq: &[Complex<f32>], syncs: &Syncs, out: &mut Vec<Packet>) {
        self.demod.process(iq, &mut self.frequency);
        for &value in &self.frequency {
            let slot = &mut self.tail[self.next];
            self.running += value - *slot;
            *slot = value;
            self.next = (self.next + 1) % SPS;
            let smoothed = self.running / SPS as f32;
            let index = self.history.len();
            let rising = index.checked_sub(SPS).map_or(0, |earlier| {
                self.rising[earlier] << 1 | u64::from(smoothed > self.history[earlier])
            });
            self.history.push(smoothed);
            self.rising.push(rising);
        }
        self.power.extend(iq.iter().map(Complex::norm_sqr));
        self.scan(syncs, out);
    }

    fn scan(&mut self, syncs: &Syncs, out: &mut Vec<Packet>) {
        let mut at = self.cursor;
        while at + LOOKAHEAD <= self.history.len() {
            match self.candidate(syncs, at) {
                Found::Packet(packet, end) => {
                    out.push(packet);
                    at = end;
                }
                Found::Wait => break,
                Found::Nothing => at += 1,
            }
        }
        self.cursor = at;
        self.trim();
    }

    fn candidate(&mut self, syncs: &Syncs, at: usize) -> Found {
        let one = gfsk::screen(self.rising[at + gfsk::SCREEN_END]);
        let long_range = coded::screen(self.rising[at + coded::SCREEN_END]);
        if !one && !long_range {
            return Found::Nothing;
        }
        if let Some(level) = one.then(|| syncs.one.check(&self.history, at)).flatten() {
            let (at, level) = self.refine(at, level, |history, start, level| {
                syncs.one.score(history, start, level)
            });
            let mut reader = Reader::new(&self.history, (at + gfsk::SYNC_SAMPLES) as f64, level);
            match gfsk::read_pdu(&mut reader, self.channel_index) {
                Read::Pdu(pdu) => {
                    let end = reader.position() as usize;
                    return Found::Packet(self.packet(pdu, RemoteIdPhy::Le1m, at, end), end);
                }
                Read::Short => return Found::Wait,
                Read::Bad => {}
            }
        }
        if let Some(level) = long_range
            .then(|| syncs.coded.check(&self.history, at))
            .flatten()
        {
            let (at, level) = self.refine(at, level, |history, start, level| {
                syncs.coded.score(history, start, level)
            });
            let mut reader = Reader::new(&self.history, (at + coded::SYNC_SAMPLES) as f64, level);
            match syncs.coded.read(&mut reader, self.channel_index) {
                Ok((phy, pdu)) => {
                    let end = reader.position() as usize;
                    return Found::Packet(self.packet(pdu, phy, at, end), end);
                }
                Err(Read::Short) => return Found::Wait,
                Err(_) => {}
            }
        }
        Found::Nothing
    }

    fn refine(
        &self,
        at: usize,
        level: Level,
        score: impl Fn(&[f32], usize, Level) -> f32,
    ) -> (usize, Level) {
        let last = self.history.len().saturating_sub(LOOKAHEAD);
        let best = (at.saturating_sub(REFINE)..=(at + REFINE).min(last))
            .max_by(|&a, &b| {
                score(&self.history, a, level).total_cmp(&score(&self.history, b, level))
            })
            .unwrap_or(at);
        (best, level)
    }

    fn packet(&self, pdu: Vec<u8>, phy: RemoteIdPhy, start: usize, end: usize) -> Packet {
        let end = end.min(self.power.len()).max(start + 1);
        let window = &self.power[start.min(end - 1)..end];
        let mean = window.iter().sum::<f32>() / window.len() as f32;
        Packet {
            pdu,
            phy,
            rf: self.rf,
            level_dbfs: 10.0 * mean.max(1e-20).log10(),
            sample: self.consumed + start as u64,
        }
    }

    fn trim(&mut self) {
        let drop = self
            .cursor
            .max(self.history.len().saturating_sub(MAX_HISTORY));
        if drop == 0 {
            return;
        }
        self.history.drain(..drop);
        self.rising.drain(..drop);
        self.power.drain(..drop);
        self.consumed += drop as u64;
        self.cursor = self.cursor.saturating_sub(drop);
    }
}
