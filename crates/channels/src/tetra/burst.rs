use super::coding::Soft;

pub(crate) const BURST_BITS: usize = 510;

pub(crate) const NORMAL_1: [u8; 22] = [
    1, 1, 0, 1, 0, 0, 0, 0, 1, 1, 1, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 0,
];
pub(crate) const NORMAL_2: [u8; 22] = [
    0, 1, 1, 1, 1, 0, 1, 0, 0, 1, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1, 1, 0,
];
#[cfg(any(test, feature = "synth"))]
pub(crate) const NORMAL_3: [u8; 22] = [
    1, 0, 1, 1, 0, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 0, 1, 0, 1, 1, 0, 1,
];
pub(crate) const SYNC_TRAINING: [u8; 38] = [
    1, 1, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 0, 1, 0, 0, 1, 1, 1, 0, 0, 0, 0, 0, 1,
    1, 0, 0, 1, 1, 1,
];

#[cfg(any(test, feature = "synth"))]
pub(crate) const HEAD_BITS: usize = 12;

pub(crate) const FREQUENCY_AT: usize = 14;
pub(crate) const FREQUENCY_BITS: usize = 80;
pub(crate) const SB1_AT: usize = 94;
pub(crate) const SYNC_AT: usize = 214;
#[cfg(any(test, feature = "synth"))]
pub(crate) const SYNC_AACH_AT: usize = 252;
pub(crate) const BLOCK1_AT: usize = 14;
pub(crate) const AACH1_AT: usize = 230;
pub(crate) const AACH1_BITS: usize = 14;
pub(crate) const NORMAL_AT: usize = 244;
pub(crate) const AACH2_AT: usize = 266;
pub(crate) const BLOCK2_AT: usize = 282;
pub(crate) const HALF_BITS: usize = 216;
pub(crate) const SB1_BITS: usize = 120;
#[cfg(any(test, feature = "synth"))]
pub(crate) const TAIL_AT: usize = 500;

pub(crate) fn frequency_correction(i: usize) -> u8 {
    u8::from(!(8..72).contains(&i))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BurstKind {
    Sync,
    Normal,
    Split,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SyncEvent {
    Candidate,
    Burst(BurstKind),
    Missed,
    Lost,
}

const RING: usize = 2_048;
const SYNC_MASK: u64 = (1 << SYNC_TRAINING.len()) - 1;
const HUNT_SYNC_ERRORS: u32 = 4;
const HUNT_FREQUENCY_ERRORS: u32 = 10;
const LOCK_SYNC_ERRORS: u32 = 7;
const LOCK_NORMAL_ERRORS: u32 = 4;
const SLIP_BITS: usize = 4;
const MAX_MISSES: u32 = 36;

pub(crate) struct Synchronizer {
    ring: Box<[Soft; RING]>,
    written: usize,
    register: u64,
    flipped: u64,
    inverted: bool,
    next: Option<usize>,
    misses: u32,
    candidate: usize,
    window: Box<[Soft; BURST_BITS]>,
}

impl Synchronizer {
    pub(crate) fn new() -> Self {
        Self {
            ring: Box::new([0; RING]),
            written: 0,
            register: 0,
            flipped: 0,
            inverted: false,
            next: None,
            misses: 0,
            candidate: 0,
            window: Box::new([0; BURST_BITS]),
        }
    }

    pub(crate) fn reset(&mut self) {
        self.written = 0;
        self.register = 0;
        self.flipped = 0;
        self.next = None;
        self.misses = 0;
    }

    pub(crate) fn window(&self) -> &[Soft; BURST_BITS] {
        &self.window
    }

    pub(crate) fn push(&mut self, first: Soft, second: Soft) -> Option<SyncEvent> {
        self.write(first, true);
        self.write(second, false);
        match self.next {
            None => self.hunt(),
            Some(start) if self.written >= start + BURST_BITS + SLIP_BITS => {
                Some(self.track(start))
            }
            Some(_) => None,
        }
    }

    pub(crate) fn confirm(&mut self, inverted: bool) {
        self.inverted = inverted;
        self.next = Some(self.candidate);
        self.misses = 0;
    }

    pub(crate) fn candidate_inverted(&self) -> bool {
        self.flipped_distance() < self.direct_distance()
    }

    pub(crate) fn candidate_bits(&self, at: usize, out: &mut [Soft], inverted: bool) {
        copy(&self.ring, self.candidate + at, out, inverted);
    }

    fn write(&mut self, soft: Soft, first: bool) {
        self.ring[self.written % RING] = soft;
        let bit = u64::from(soft > 0);
        self.register = self.register << 1 | bit;
        self.flipped = self.flipped << 1 | (bit ^ u64::from(first));
        self.written += 1;
    }

    fn direct_distance(&self) -> u32 {
        ((self.register ^ pattern(&SYNC_TRAINING)) & SYNC_MASK).count_ones()
    }

    fn flipped_distance(&self) -> u32 {
        ((self.flipped ^ pattern(&SYNC_TRAINING)) & SYNC_MASK).count_ones()
    }

    fn hunt(&mut self) -> Option<SyncEvent> {
        let end = SYNC_AT + SYNC_TRAINING.len();
        if self.written < end || !self.written.is_multiple_of(2) {
            return None;
        }
        let inverted = self.candidate_inverted();
        let distance = self.direct_distance().min(self.flipped_distance());
        if distance > HUNT_SYNC_ERRORS {
            return None;
        }
        let start = self.written - end;
        let frequency = self.distance(
            start + FREQUENCY_AT,
            FREQUENCY_BITS,
            frequency_correction,
            inverted,
        );
        if frequency > HUNT_FREQUENCY_ERRORS {
            return None;
        }
        self.candidate = start;
        Some(SyncEvent::Candidate)
    }

    fn track(&mut self, start: usize) -> SyncEvent {
        let mut best: Option<(u32, usize, BurstKind)> = None;
        for slip in [0isize, -2, 2, -4, 4] {
            let Some(at) = start.checked_add_signed(slip) else {
                continue;
            };
            if let Some((score, kind)) = self.classify(at)
                && best.is_none_or(|(best_score, _, _)| score < best_score)
            {
                best = Some((score, at, kind));
            }
        }
        match best {
            Some((_, at, kind)) => {
                self.misses = 0;
                self.fill_window(at);
                self.next = Some(at + BURST_BITS);
                SyncEvent::Burst(kind)
            }
            None => {
                self.misses += 1;
                self.next = Some(start + BURST_BITS);
                if self.misses > MAX_MISSES {
                    self.next = None;
                    return SyncEvent::Lost;
                }
                SyncEvent::Missed
            }
        }
    }

    fn classify(&self, start: usize) -> Option<(u32, BurstKind)> {
        let inverted = self.inverted;
        let sync = self.distance(
            start + SYNC_AT,
            SYNC_TRAINING.len(),
            |i| SYNC_TRAINING[i],
            inverted,
        );
        let normal = self.distance(start + NORMAL_AT, NORMAL_1.len(), |i| NORMAL_1[i], inverted);
        let split = self.distance(start + NORMAL_AT, NORMAL_2.len(), |i| NORMAL_2[i], inverted);
        [
            (sync <= LOCK_SYNC_ERRORS).then_some((sync * 22 / 38, BurstKind::Sync)),
            (normal <= LOCK_NORMAL_ERRORS).then_some((normal, BurstKind::Normal)),
            (split <= LOCK_NORMAL_ERRORS).then_some((split, BurstKind::Split)),
        ]
        .into_iter()
        .flatten()
        .min_by_key(|&(score, _)| score)
    }

    fn distance(
        &self,
        at: usize,
        len: usize,
        expected: impl Fn(usize) -> u8,
        inverted: bool,
    ) -> u32 {
        (0..len)
            .map(|i| {
                let bit = self.hard(at + i, inverted);
                u32::from(bit != (expected(i) == 1))
            })
            .sum()
    }

    fn hard(&self, at: usize, inverted: bool) -> bool {
        (self.ring[at % RING] > 0) ^ (inverted && at.is_multiple_of(2))
    }

    fn fill_window(&mut self, start: usize) {
        copy(&self.ring, start, &mut self.window[..], self.inverted);
    }
}

fn copy(ring: &[Soft; RING], at: usize, out: &mut [Soft], inverted: bool) {
    for (i, slot) in out.iter_mut().enumerate() {
        let index = at + i;
        let soft = ring[index % RING];
        *slot = if inverted && index.is_multiple_of(2) {
            -soft
        } else {
            soft
        };
    }
}

const fn pattern(bits: &[u8]) -> u64 {
    let mut value = 0u64;
    let mut i = 0;
    while i < bits.len() {
        value = value << 1 | bits[i] as u64;
        i += 1;
    }
    value
}
