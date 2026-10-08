use super::{ACCESS_ADDRESS, CRC_BYTES, HEADER_BYTES, Whitener};

pub(crate) const SPS: usize = 4;
pub(crate) const RATE_HZ: f64 = 4_000_000.0;
pub(crate) const DEVIATION_HZ: f64 = 250_000.0;
const PREAMBLE_BITS: usize = 8;
pub(crate) const SYNC_BITS: usize = PREAMBLE_BITS + 32;
pub(crate) const SYNC_SAMPLES: usize = SYNC_BITS * SPS;
const SYNC_ERRORS: usize = 3;
const MIN_SWING: f32 = 0.25;
const TIMING_GAIN: f64 = 0.06;
const MAX_TIMING_STEP: f64 = 0.5;

pub(crate) const SCREEN_END: usize = (PREAMBLE_BITS - 1) * SPS;
const SCREEN_MASK: u64 = 0x7F;
const SCREEN_ALTERNATION: u64 = 0x55;

pub(crate) fn screen(rising: u64) -> bool {
    rising & SCREEN_MASK == SCREEN_ALTERNATION
}

pub(crate) fn access_address_bits() -> impl Iterator<Item = bool> {
    (0..32).map(|bit| ACCESS_ADDRESS >> bit & 1 == 1)
}

fn sync_pattern() -> [bool; SYNC_BITS] {
    let mut pattern = [false; SYNC_BITS];
    for (slot, bit) in pattern.iter_mut().zip(
        (0..PREAMBLE_BITS)
            .map(|bit| bit % 2 == 1)
            .chain(access_address_bits()),
    ) {
        *slot = bit;
    }
    pattern
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Level {
    pub dc: f32,
    pub swing: f32,
}

impl Level {
    pub(crate) fn fit(values: impl Iterator<Item = (f32, bool)>) -> Option<Self> {
        let (mut count, mut sum, mut signed, mut ones) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        for (value, bit) in values {
            let sign = if bit { 1.0 } else { -1.0 };
            count += 1.0;
            sum += value;
            signed += value * sign;
            ones += sign;
        }
        let denominator = count * count - ones * ones;
        if count == 0.0 || denominator.abs() < f32::EPSILON {
            return None;
        }
        let swing = (count * signed - ones * sum) / denominator;
        let dc = (sum - swing * ones) / count;
        Some(Self { dc, swing })
    }
}

pub(crate) struct Reader<'a> {
    history: &'a [f32],
    position: f64,
    level: Level,
    previous: Option<f32>,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(history: &'a [f32], position: f64, level: Level) -> Self {
        Self {
            history,
            position,
            level,
            previous: None,
        }
    }

    pub(crate) fn remaining(&self) -> usize {
        let last = self.history.len().saturating_sub(2) as f64;
        ((last - self.position) / SPS as f64).max(0.0) as usize
    }

    pub(crate) fn position(&self) -> f64 {
        self.position
    }

    pub(crate) fn symbol(&mut self) -> Option<f32> {
        let at = self.position.floor();
        let index = at as usize;
        let weight = (self.position - at) as f32;
        let (&left, &right) = (self.history.get(index)?, self.history.get(index + 1)?);
        let value = ((left * (1.0 - weight) + right * weight) - self.level.dc) / self.level.swing;
        if let Some(previous) = self.previous
            && (previous > 0.0) != (value > 0.0)
        {
            self.track(index);
        }
        self.previous = Some(value);
        self.position += SPS as f64;
        Some(value)
    }

    fn track(&mut self, index: usize) {
        let expected = self.position - SPS as f64 / 2.0;
        let first = index.saturating_sub(SPS);
        let level = |j: usize| self.history[j] - self.level.dc;
        let crossing = (first..index).find_map(|j| {
            let (a, b) = (level(j), level(j + 1));
            ((a > 0.0) != (b > 0.0)).then(|| j as f64 + f64::from(a / (a - b)))
        });
        if let Some(crossing) = crossing {
            let error = (crossing - expected).clamp(-MAX_TIMING_STEP, MAX_TIMING_STEP);
            self.position += TIMING_GAIN * error;
        }
    }

    pub(crate) fn byte(&mut self) -> Option<u8> {
        (0..8).try_fold(0u8, |acc, bit| {
            Some(acc | u8::from(self.symbol()? > 0.0) << bit)
        })
    }
}

pub(crate) struct Sync1m {
    pattern: [bool; SYNC_BITS],
}

impl Sync1m {
    pub(crate) fn new() -> Self {
        Self {
            pattern: sync_pattern(),
        }
    }

    pub(crate) fn check(&self, history: &[f32], at: usize) -> Option<Level> {
        let sample = |bit: usize| history[at + bit * SPS];
        let alternates = (0..PREAMBLE_BITS - 1)
            .all(|bit| (sample(bit) < sample(bit + 1)) == self.pattern[bit + 1]);
        if !alternates {
            return None;
        }
        let mean = (0..PREAMBLE_BITS).map(sample).sum::<f32>() / PREAMBLE_BITS as f32;
        if (0..PREAMBLE_BITS).any(|bit| (sample(bit) > mean) != self.pattern[bit]) {
            return None;
        }
        let errors = (PREAMBLE_BITS..SYNC_BITS)
            .filter(|&bit| (sample(bit) > mean) != self.pattern[bit])
            .count();
        if errors > SYNC_ERRORS {
            return None;
        }
        let level = Level::fit((0..SYNC_BITS).map(|bit| (sample(bit), self.pattern[bit])))?;
        (level.swing > MIN_SWING).then_some(level)
    }

    pub(crate) fn score(&self, history: &[f32], at: usize, level: Level) -> f32 {
        (0..SYNC_BITS)
            .map(|bit| {
                let value = history[at + bit * SPS] - level.dc;
                if self.pattern[bit] { value } else { -value }
            })
            .sum()
    }
}

pub(crate) enum Read {
    Pdu(Vec<u8>),
    Short,
    Bad,
}

pub(crate) fn read_pdu(reader: &mut Reader<'_>, channel_index: u8) -> Read {
    if reader.remaining() < HEADER_BYTES * 8 {
        return Read::Short;
    }
    let mut whitener = Whitener::new(channel_index);
    let mut pdu = Vec::with_capacity(64);
    for _ in 0..HEADER_BYTES {
        let Some(byte) = reader.byte() else {
            return Read::Short;
        };
        pdu.push(whitener.byte(byte));
    }
    let total = HEADER_BYTES + usize::from(pdu[1]) + CRC_BYTES;
    if reader.remaining() < (total - HEADER_BYTES) * 8 {
        return Read::Short;
    }
    for _ in HEADER_BYTES..total {
        let Some(byte) = reader.byte() else {
            return Read::Short;
        };
        pdu.push(whitener.byte(byte));
    }
    if super::crc_ok(&pdu) {
        pdu.truncate(total - CRC_BYTES);
        Read::Pdu(pdu)
    } else {
        Read::Bad
    }
}
