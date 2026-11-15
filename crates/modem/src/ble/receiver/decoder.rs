use num_complex::Complex;

use super::{
    super::{
        BlePhy, CRC_BYTES, HEADER_BYTES, MAX_PDU_BYTES, SPS, Whitener,
        coded::{INDICATOR_BLOCKS, S8_SYMBOLS, TERM_BITS, Viterbi},
        crc_ok,
        track::{Acquired, Blocks, Coherent, acquire, carrier_from_pilot, smoothed_soft},
    },
    AA_SYMBOLS, Attempt, BLOCK_SAMPLES, CODED_SEGMENT, ONE_SEGMENT, ONE_SYMBOLS,
    patterns::Patterns,
};

const MIN_COHERENCE: f32 = 0.6;
const HEADER_BITS: usize = HEADER_BYTES * 8;
const HEADER_MARGIN_BITS: usize = 24;
const MARGIN_SAMPLES: f64 = (2 * BLOCK_SAMPLES) as f64;

pub(super) struct Scratch {
    pub(super) pdu: [u8; MAX_PDU_BYTES + CRC_BYTES],
    pilots: Vec<Complex<f32>>,
    data: Vec<Complex<f32>>,
    soft: Vec<f32>,
    bits: Vec<bool>,
    viterbi: Viterbi,
}

impl Scratch {
    pub(super) fn new() -> Self {
        let coded = 2 * ((MAX_PDU_BYTES + CRC_BYTES) * 8 + TERM_BITS) + INDICATOR_BLOCKS;
        Self {
            pdu: [0; MAX_PDU_BYTES + CRC_BYTES],
            pilots: Vec::with_capacity(coded),
            data: Vec::with_capacity(coded),
            soft: Vec::with_capacity(coded),
            bits: Vec::with_capacity(coded),
            viterbi: Viterbi::new(),
        }
    }
}

enum Outcome {
    Good(usize, usize),
    Corrupt(usize),
}

type Stage = Result<Option<Outcome>, usize>;

pub(super) struct Decoder<'a> {
    pub(super) y: &'a [Complex<f32>],
    pub(super) channel_index: u8,
    pub(super) patterns: &'a Patterns,
    pub(super) scratch: &'a mut Scratch,
}

impl Decoder<'_> {
    pub(super) fn one(&mut self, anchor: usize) -> Attempt {
        let Some(acquired) = acquire(self.y, anchor, &self.patterns.one_known, ONE_SEGMENT)
            .filter(|acquired| acquired.coherence >= MIN_COHERENCE)
        else {
            return Attempt::Miss;
        };
        let start = anchor.saturating_sub(ONE_SYMBOLS * SPS);
        let view = acquired.view(self.y);
        let mut detector = Coherent::new(view, acquired.instant, acquired.carrier, acquired.last);
        if let Err(needed) = self.need(detector.position(), HEADER_BITS) {
            return Attempt::Wait(needed);
        }
        let mut whitener = Whitener::new(self.channel_index);
        for slot in 0..HEADER_BYTES {
            let Some(byte) = read_byte(&mut detector) else {
                return Attempt::Wait(self.more());
            };
            self.scratch.pdu[slot] = whitener.byte(byte);
        }
        let total = HEADER_BYTES + usize::from(self.scratch.pdu[1]) + CRC_BYTES;
        if let Err(needed) = self.need(detector.position(), (total - HEADER_BYTES) * 8) {
            return Attempt::Wait(needed);
        }
        for slot in HEADER_BYTES..total {
            let Some(byte) = read_byte(&mut detector) else {
                return Attempt::Wait(self.more());
            };
            self.scratch.pdu[slot] = whitener.byte(byte);
        }
        if crc_ok(&self.scratch.pdu[..total]) {
            Attempt::Done(
                start,
                detector.position() as usize,
                BlePhy::Le1m,
                total - CRC_BYTES,
            )
        } else {
            Attempt::Corrupt(detector.position() as usize)
        }
    }

    fn need(&self, position: f64, symbols: usize) -> Result<(), usize> {
        let needed = (position + (symbols * SPS) as f64 + MARGIN_SAMPLES).ceil() as usize + 1;
        if needed <= self.y.len() {
            Ok(())
        } else {
            Err(needed)
        }
    }

    fn more(&self) -> usize {
        self.y.len() + BLOCK_SAMPLES
    }

    pub(super) fn coded(&mut self, anchor: usize) -> Attempt {
        let Some(acquired) = acquire(self.y, anchor, &self.patterns.coded_known, CODED_SEGMENT)
            .filter(|acquired| acquired.coherence >= MIN_COHERENCE)
        else {
            return Attempt::Miss;
        };
        let header_coded = 2 * (HEADER_BITS + HEADER_MARGIN_BITS);
        if let Err(needed) = self.need(
            acquired.instant,
            (INDICATOR_BLOCKS + header_coded) * S8_SYMBOLS,
        ) {
            return Attempt::Wait(needed);
        }
        let view = acquired.view(self.y);
        let mut blocks = Blocks::new(
            view,
            acquired.instant,
            acquired.carrier * acquired.last * 2.0,
        );
        self.scratch.pilots.clear();
        self.scratch.data.clear();
        if !read_blocks(&mut blocks, INDICATOR_BLOCKS, self.scratch) {
            return Attempt::Wait(self.more());
        }
        let phy = self.indicator();
        let start = anchor.saturating_sub(AA_SYMBOLS * SPS);
        let outcome = match phy {
            BlePhy::CodedS8 => self.s8(&mut blocks, header_coded),
            _ => self.s2(&acquired, &blocks, header_coded),
        };
        match outcome {
            Ok(Some(Outcome::Good(end, length))) => Attempt::Done(start, end, phy, length),
            Ok(Some(Outcome::Corrupt(end))) => Attempt::Corrupt(end),
            Ok(None) => Attempt::Miss,
            Err(needed) => Attempt::Wait(needed),
        }
    }

    fn indicator(&mut self) -> BlePhy {
        self.scratch.soft.clear();
        smoothed_soft(
            &self.scratch.pilots,
            &self.scratch.data,
            &mut self.scratch.soft,
        );
        self.patterns
            .indicators
            .iter()
            .map(|(phy, expected)| {
                let score: f32 = self
                    .scratch
                    .soft
                    .iter()
                    .zip(expected)
                    .map(|(&soft, &bit)| if bit { soft } else { -soft })
                    .sum();
                (*phy, score)
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(BlePhy::CodedS8, |(phy, _)| phy)
    }

    fn s8(&mut self, blocks: &mut Blocks<'_>, header_coded: usize) -> Stage {
        if !read_blocks(blocks, header_coded, self.scratch) {
            return Err(self.more());
        }
        let length = self.coded_length(true).ok_or_else(|| self.more())?;
        let coded = 2 * ((HEADER_BYTES + length + CRC_BYTES) * 8 + TERM_BITS);
        let remaining = coded.saturating_sub(header_coded);
        self.need(blocks.position(), remaining * S8_SYMBOLS)?;
        if !read_blocks(blocks, remaining, self.scratch) {
            return Err(self.more());
        }
        self.scratch.soft.clear();
        smoothed_soft(
            &self.scratch.pilots,
            &self.scratch.data,
            &mut self.scratch.soft,
        );
        Ok(self.finish(INDICATOR_BLOCKS, coded, length, blocks.position()))
    }

    fn s2(&mut self, acquired: &Acquired, blocks: &Blocks<'_>, header_coded: usize) -> Stage {
        let carrier = carrier_from_pilot(blocks.reference(), acquired.last);
        let view = acquired.view(self.y);
        let mut detector = Coherent::new(view, blocks.position(), carrier, acquired.last);
        self.scratch.soft.clear();
        for _ in 0..header_coded {
            let soft = detector.symbol().ok_or_else(|| self.more())?;
            self.scratch.soft.push(soft);
        }
        let length = self.coded_length(false).ok_or_else(|| self.more())?;
        let coded = 2 * ((HEADER_BYTES + length + CRC_BYTES) * 8 + TERM_BITS);
        let remaining = coded.saturating_sub(header_coded);
        self.need(detector.position(), remaining)?;
        for _ in 0..remaining {
            let soft = detector.symbol().ok_or_else(|| self.more())?;
            self.scratch.soft.push(soft);
        }
        Ok(self.finish(0, coded, length, detector.position()))
    }

    fn coded_length(&mut self, smooth: bool) -> Option<usize> {
        let first = if smooth {
            self.scratch.soft.clear();
            smoothed_soft(
                &self.scratch.pilots,
                &self.scratch.data,
                &mut self.scratch.soft,
            );
            INDICATOR_BLOCKS
        } else {
            0
        };
        let scratch = &mut self.scratch;
        let header_coded = 2 * (HEADER_BITS + HEADER_MARGIN_BITS);
        scratch.bits.clear();
        scratch.viterbi.decode(
            scratch.soft.get(first..first + header_coded)?,
            false,
            &mut scratch.bits,
        );
        let mut whitener = Whitener::new(self.channel_index);
        whitener.byte(byte_of(&scratch.bits[..8]));
        Some(usize::from(whitener.byte(byte_of(&scratch.bits[8..16]))))
    }

    fn finish(&mut self, first: usize, coded: usize, length: usize, end: f64) -> Option<Outcome> {
        let scratch = &mut self.scratch;
        let soft = scratch.soft.get(first..first + coded)?;
        scratch.bits.clear();
        scratch.viterbi.decode(soft, true, &mut scratch.bits);
        let total = HEADER_BYTES + length + CRC_BYTES;
        let mut whitener = Whitener::new(self.channel_index);
        for (slot, bits) in scratch.pdu[..total]
            .iter_mut()
            .zip(scratch.bits.as_chunks::<8>().0)
        {
            *slot = whitener.byte(byte_of(bits));
        }
        Some(if crc_ok(&scratch.pdu[..total]) {
            Outcome::Good(end as usize, total - CRC_BYTES)
        } else {
            Outcome::Corrupt(end as usize)
        })
    }
}

fn read_blocks(blocks: &mut Blocks<'_>, count: usize, scratch: &mut Scratch) -> bool {
    for _ in 0..count {
        let Some((pilot, data)) = blocks.block() else {
            return false;
        };
        scratch.pilots.push(pilot);
        scratch.data.push(data);
    }
    true
}

fn read_byte(detector: &mut Coherent<'_>) -> Option<u8> {
    (0..8).try_fold(0u8, |acc, bit| {
        Some(acc | u8::from(detector.symbol()? > 0.0) << bit)
    })
}

fn byte_of(bits: &[bool]) -> u8 {
    bits.iter()
        .enumerate()
        .fold(0u8, |acc, (bit, &value)| acc | u8::from(value) << bit)
}
