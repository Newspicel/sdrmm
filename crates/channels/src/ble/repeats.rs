const REMEMBERED: usize = 512;
const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

#[derive(Clone, Copy, Default)]
struct Seen {
    key: u64,
    content: u64,
    last: u64,
    quiet: u32,
}

pub(crate) struct Repeats {
    seen: Vec<Seen>,
    hold: u64,
}

impl Repeats {
    pub(crate) fn new(hold: u64) -> Self {
        Self {
            seen: Vec::with_capacity(REMEMBERED),
            hold,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.seen.clear();
    }

    pub(crate) fn admit(&mut self, key: u64, content: u64, now: u64) -> Option<u32> {
        if let Some(seen) = self.seen.iter_mut().find(|seen| seen.key == key) {
            let fresh = seen.content != content || now < seen.last || now - seen.last >= self.hold;
            if !fresh {
                seen.quiet = seen.quiet.saturating_add(1);
                return None;
            }
            let quiet = seen.quiet;
            *seen = Seen {
                key,
                content,
                last: now,
                quiet: 0,
            };
            return Some(quiet);
        }
        let entry = Seen {
            key,
            content,
            last: now,
            quiet: 0,
        };
        if self.seen.len() < REMEMBERED {
            self.seen.push(entry);
        } else if let Some(oldest) = self.seen.iter_mut().min_by_key(|seen| seen.last) {
            *oldest = entry;
        }
        Some(0)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Fnv(u64);

impl Fnv {
    pub(crate) fn new() -> Self {
        Self(FNV_OFFSET)
    }

    pub(crate) fn bytes(self, bytes: &[u8]) -> Self {
        Self(bytes.iter().fold(self.0, |hash, &byte| {
            (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME)
        }))
    }

    pub(crate) fn finish(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_repeating_itself_is_held_until_its_content_or_the_hold_changes() {
        let mut repeats = Repeats::new(100);
        assert_eq!(repeats.admit(1, 10, 0), Some(0));
        assert_eq!(repeats.admit(1, 10, 40), None);
        assert_eq!(repeats.admit(1, 10, 60), None);
        assert_eq!(repeats.admit(1, 11, 70), Some(2));
        assert_eq!(repeats.admit(1, 11, 169), None);
        assert_eq!(repeats.admit(1, 11, 170), Some(1));
        assert_eq!(repeats.admit(2, 11, 171), Some(0));
    }

    #[test]
    fn the_oldest_device_makes_room() {
        let mut repeats = Repeats::new(1_000);
        for key in 0..REMEMBERED as u64 {
            repeats.admit(key, 0, key);
        }
        assert_eq!(repeats.admit(9_999, 0, 600), Some(0));
        assert_eq!(repeats.admit(0, 0, 601), Some(0));
        assert_eq!(repeats.admit(5, 0, 602), None);
    }
}
