use super::coding::{FRAME_BITS, SYNC, SYNC_BITS};

const HISTORY: usize = 512;
const WINDOW_MASK: u16 = 0x7F;
const LOCK_STREAK: u8 = 4;
const SYNC_TOLERANCE: u32 = 2;
const MAX_MISSES: u8 = 10;
const UNCONFIRMED_MISSES: u8 = 2;
const CONFIRM_FRAMES: u8 = 12;
const SHUNNED: usize = 4;
const SHUN_SYMBOLS: usize = 250 * FRAME_BITS;
const LOOKBACK: usize = FRAME_BITS + SYNC_BITS - 1;

const fn sync_pattern() -> u16 {
    let mut pattern = 0;
    let mut k = 1;
    while k < SYNC_BITS {
        pattern = pattern << 1 | (SYNC[k] ^ SYNC[k - 1]) as u16;
        k += 1;
    }
    pattern
}

const PATTERN: u16 = sync_pattern();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Polarity {
    Normal,
    Inverted,
}

impl Polarity {
    const ALL: [Self; 2] = [Self::Normal, Self::Inverted];

    fn pattern(self) -> u16 {
        match self {
            Self::Normal => PATTERN,
            Self::Inverted => !PATTERN & WINDOW_MASK,
        }
    }

    fn sign(self) -> f32 {
        match self {
            Self::Normal => 1.0,
            Self::Inverted => -1.0,
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Normal => 0,
            Self::Inverted => 1,
        }
    }
}

#[derive(Clone, Copy)]
struct Lock {
    phase: usize,
    polarity: Polarity,
    misses: u8,
    unconfirmed: u8,
    confirmed: bool,
}

#[derive(Clone, Copy)]
struct Shunned {
    phase: usize,
    polarity: Polarity,
    until: usize,
}

impl Shunned {
    fn covers(&self, phase: usize, polarity: Polarity, now: usize) -> bool {
        let apart = (phase + FRAME_BITS - self.phase) % FRAME_BITS;
        now < self.until && polarity == self.polarity && (apart <= 1 || apart == FRAME_BITS - 1)
    }
}

pub(crate) struct Framed {
    pub(crate) frame: [f32; FRAME_BITS],
    pub(crate) sync_errors: u32,
}

pub(crate) struct Framer {
    history: [f32; HISTORY],
    bits: u16,
    count: usize,
    streaks: [[u8; FRAME_BITS]; 2],
    lock: Option<Lock>,
    shunned: [Option<Shunned>; SHUNNED],
    next_shunned: usize,
}

impl Framer {
    pub(crate) fn new() -> Self {
        Self {
            history: [0.0; HISTORY],
            bits: 0,
            count: 0,
            streaks: [[0; FRAME_BITS]; 2],
            lock: None,
            shunned: [None; SHUNNED],
            next_shunned: 0,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.bits = 0;
        self.unlock();
        self.shunned = [None; SHUNNED];
    }

    fn unlock(&mut self) {
        self.streaks = [[0; FRAME_BITS]; 2];
        self.lock = None;
    }

    fn shun(&mut self, lock: Lock, now: usize) {
        self.shunned[self.next_shunned] = Some(Shunned {
            phase: lock.phase,
            polarity: lock.polarity,
            until: now + SHUN_SYMBOLS,
        });
        self.next_shunned = (self.next_shunned + 1) % SHUNNED;
        self.unlock();
    }

    pub(crate) fn locked(&self) -> bool {
        self.lock.is_some()
    }

    pub(crate) fn confirm(&mut self) {
        if let Some(lock) = &mut self.lock {
            lock.confirmed = true;
        }
    }

    pub(crate) fn push(&mut self, soft: f32) -> Option<Framed> {
        let now = self.count;
        self.history[now % HISTORY] = soft;
        self.bits = self.bits << 1 | u16::from(soft < 0.0);
        self.count += 1;
        match self.lock {
            None => self.hunt(now),
            Some(lock) => self.track(now, lock),
        }
    }

    fn distance(&self, back: usize, polarity: Polarity) -> u32 {
        ((self.bits >> back ^ polarity.pattern()) & WINDOW_MASK).count_ones()
    }

    fn hunt(&mut self, now: usize) -> Option<Framed> {
        let phase = now % FRAME_BITS;
        for polarity in Polarity::ALL {
            let shunned = self
                .shunned
                .iter()
                .flatten()
                .any(|shunned| shunned.covers(phase, polarity, now));
            let matched = !shunned && self.distance(0, polarity) == 0;
            let streak = &mut self.streaks[polarity.index()][phase];
            *streak = if matched { streak.saturating_add(1) } else { 0 };
            if *streak >= LOCK_STREAK {
                self.lock = Some(Lock {
                    phase,
                    polarity,
                    misses: 0,
                    unconfirmed: 0,
                    confirmed: false,
                });
                return None;
            }
        }
        None
    }

    fn track(&mut self, now: usize, mut lock: Lock) -> Option<Framed> {
        if (now + FRAME_BITS - lock.phase) % FRAME_BITS != 1 {
            return None;
        }
        let expected = self.distance(1, lock.polarity);
        let (end, errors) = if expected <= SYNC_TOLERANCE {
            (now - 1, expected)
        } else if self.distance(2, lock.polarity) == 0 {
            (now - 2, 0)
        } else if self.distance(0, lock.polarity) == 0 {
            (now, 0)
        } else {
            (now - 1, expected)
        };
        if errors > SYNC_TOLERANCE {
            lock.misses += 1;
        } else {
            lock.misses = 0;
        }
        let limit = if lock.confirmed {
            MAX_MISSES
        } else {
            UNCONFIRMED_MISSES
        };
        if lock.misses >= limit {
            self.unlock();
            return None;
        }
        if !lock.confirmed {
            lock.unconfirmed += 1;
            if lock.unconfirmed > CONFIRM_FRAMES {
                self.shun(lock, now);
                return None;
            }
        }
        lock.phase = end % FRAME_BITS;
        self.lock = Some(lock);
        self.framed(end, lock.polarity, errors)
    }

    fn framed(&self, end: usize, polarity: Polarity, sync_errors: u32) -> Option<Framed> {
        let start = end.checked_sub(LOOKBACK)?;
        let sign = polarity.sign();
        Some(Framed {
            frame: std::array::from_fn(|k| self.history[(start + k) % HISTORY] * sign),
            sync_errors,
        })
    }
}
