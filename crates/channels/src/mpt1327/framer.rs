//! Codeword framing: a preamble ending in SYNC or SYNT starts a message, and on a control
//! channel the stream runs on in 64-bit codewords, each slot's system codeword ending in the next
//! SYNC.

use sdrmm_dsp::SyncDetector;
use sdrmm_wire::DvFrame;

use super::codeword::{CODEWORD_BITS, Code, Codeword, PREAMBLE, SYNC, SYNT};
use super::frame::{Heard, dv_frame};
use super::message::Message;

const SYNC_BITS: u32 = 24;
const SYNC_TOLERANCE: u32 = 1;
const PREAMBLE_TAIL: u64 = PREAMBLE as u64 & 0xFF;
/// Codewords in a row that fail their check before the framing is given up.
const MAX_MISSES: u8 = 2;
const MAX_DATA: usize = 4;
const RECENT: usize = 8;
/// A message repeated within two seconds at 1200 bit/s is shown once.
const REPEAT_BITS: u64 = 2_400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lock {
    Hunting,
    Locked {
        control: bool,
        filled: u32,
        misses: u8,
        fresh: bool,
    },
}

#[derive(Clone, Copy, Debug)]
struct Pending {
    message: Message,
    info: u64,
    data: [u64; MAX_DATA],
    len: usize,
    errors: u32,
    control: bool,
}

impl Pending {
    fn complete(&self) -> bool {
        self.len >= usize::from(self.message.appended())
    }
}

pub(crate) struct Framer {
    code: Code,
    control_sync: SyncDetector,
    traffic_sync: SyncDetector,
    lock: Lock,
    word: u64,
    /// The reliability of each bit of `word`, the newest at index 0.
    reliability: [f32; CODEWORD_BITS],
    pending: Option<Pending>,
    system: Option<u16>,
    recent: [(u64, u64); RECENT],
    next_recent: usize,
    clock: u64,
    rejected: u32,
}

impl Framer {
    pub(crate) fn new() -> Self {
        Self {
            code: Code::new(),
            control_sync: SyncDetector::new(
                PREAMBLE_TAIL << 16 | u64::from(SYNC),
                SYNC_BITS,
                SYNC_TOLERANCE,
            ),
            traffic_sync: SyncDetector::new(
                PREAMBLE_TAIL << 16 | u64::from(SYNT),
                SYNC_BITS,
                SYNC_TOLERANCE,
            ),
            lock: Lock::Hunting,
            word: 0,
            reliability: [0.0; CODEWORD_BITS],
            pending: None,
            system: None,
            recent: [(u64::MAX, 0); RECENT],
            next_recent: 0,
            clock: 0,
            rejected: 0,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.control_sync.reset();
        self.traffic_sync.reset();
        self.lock = Lock::Hunting;
        if self.pending.take().is_some() {
            self.rejected = self.rejected.saturating_add(1);
        }
        self.system = None;
        self.recent = [(u64::MAX, 0); RECENT];
    }

    /// Codewords that failed their check right after a sync, and messages cut short.
    pub(crate) const fn rejected(&self) -> u32 {
        self.rejected
    }

    /// Takes one soft bit, positive for a one.
    pub(crate) fn push(&mut self, soft: f32, frames: &mut Vec<DvFrame>) {
        let bit = soft > 0.0;
        self.clock += 1;
        self.word = self.word << 1 | u64::from(bit);
        self.reliability.copy_within(..CODEWORD_BITS - 1, 1);
        self.reliability[0] = soft.abs();
        let control_hit = self.control_sync.push(bit);
        let traffic_hit = self.traffic_sync.push(bit);
        if let Lock::Locked {
            control,
            filled,
            misses,
            fresh,
        } = self.lock
        {
            if filled + 1 == CODEWORD_BITS as u32 {
                self.lock = Lock::Locked {
                    control,
                    filled: 0,
                    misses,
                    fresh: false,
                };
                self.codeword(control, misses, fresh, frames);
            } else {
                self.lock = Lock::Locked {
                    control,
                    filled: filled + 1,
                    misses,
                    fresh,
                };
            }
        }
        if control_hit || traffic_hit {
            self.synced(control_hit);
        }
    }

    /// A sync starts the framing, confirms it on a codeword boundary, or moves it once the
    /// codewords where it stood stopped checking.
    fn synced(&mut self, control: bool) {
        let follows = match self.lock {
            Lock::Hunting => true,
            Lock::Locked { filled, misses, .. } => filled == 0 || misses > 0,
        };
        if follows {
            self.lock = Lock::Locked {
                control,
                filled: 0,
                misses: 0,
                fresh: true,
            };
        }
    }

    /// Ends whatever message is open, as when the signal goes.
    fn flush(&mut self, frames: &mut Vec<DvFrame>) {
        if let Some(pending) = self.pending.take() {
            self.finish(&pending, frames);
        }
    }

    fn codeword(&mut self, control: bool, misses: u8, fresh: bool, frames: &mut Vec<DvFrame>) {
        let Some(codeword) = self.code.decode_soft(self.word, &self.reliability) else {
            self.miss(misses, fresh, frames);
            return;
        };
        if let Lock::Locked { misses, .. } = &mut self.lock {
            *misses = 0;
        }
        if let Some(system) = codeword.system() {
            self.flush(frames);
            self.system = Some(system.system);
            return;
        }
        if codeword.is_address() {
            self.flush(frames);
            self.open(codeword, control, frames);
            return;
        }
        let Some(mut pending) = self.pending.take() else {
            return;
        };
        if pending.len < MAX_DATA {
            pending.data[pending.len] = codeword.info();
            pending.len += 1;
        }
        pending.errors += codeword.errors;
        if pending.complete() {
            self.finish(&pending, frames);
        } else {
            self.pending = Some(pending);
        }
    }

    fn miss(&mut self, misses: u8, fresh: bool, frames: &mut Vec<DvFrame>) {
        if fresh {
            self.rejected = self.rejected.saturating_add(1);
        }
        let misses = misses + 1;
        if misses >= MAX_MISSES {
            self.flush(frames);
            self.lock = Lock::Hunting;
        } else if let Lock::Locked { misses: held, .. } = &mut self.lock {
            *held = misses;
        }
    }

    fn open(&mut self, codeword: Codeword, control: bool, frames: &mut Vec<DvFrame>) {
        let message = Message::read(codeword.info());
        if let Some(system) = message.own_system() {
            self.system = Some(system);
        }
        let pending = Pending {
            message,
            info: codeword.info(),
            data: [0; MAX_DATA],
            len: 0,
            errors: codeword.errors,
            control,
        };
        if pending.complete() {
            self.finish(&pending, frames);
        } else {
            self.pending = Some(pending);
        }
    }

    fn finish(&mut self, pending: &Pending, frames: &mut Vec<DvFrame>) {
        if !pending.complete() {
            self.rejected = self.rejected.saturating_add(1);
        }
        if pending.len == 0 && self.repeated(pending.info) {
            return;
        }
        frames.push(dv_frame(&Heard {
            message: pending.message,
            info: pending.info,
            data: &pending.data[..pending.len],
            errors: pending.errors,
            system: self.system,
            control: pending.control,
        }));
    }

    /// Whether `info` went out within the last two seconds; remembers it either way.
    fn repeated(&mut self, info: u64) -> bool {
        let clock = self.clock;
        if let Some(seen) = self.recent.iter_mut().find(|(seen, _)| *seen == info) {
            let fresh = clock.saturating_sub(seen.1) >= REPEAT_BITS;
            if fresh {
                seen.1 = clock;
            }
            return !fresh;
        }
        self.recent[self.next_recent] = (info, clock);
        self.next_recent = (self.next_recent + 1) % RECENT;
        false
    }
}
