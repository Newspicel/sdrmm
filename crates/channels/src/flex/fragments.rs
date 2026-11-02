use sdrmm_wire::{DecoderEvent, FlexMessage};

/// The fragment number of a message's first piece; later pieces count 0, 1, 2, 0, ...
const FIRST: u8 = 3;
const MAX_PENDING: usize = 16;
pub const MAX_TEXT: usize = 1_024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Piece {
    number: u8,
    continued: bool,
}

impl Piece {
    pub const WHOLE: Self = Self {
        number: FIRST,
        continued: false,
    };

    pub fn of(header: u32) -> Self {
        Self {
            number: (header >> 11 & 3) as u8,
            continued: header >> 10 & 1 == 1,
        }
    }

    fn follows(self, previous: Self) -> bool {
        let expected = if previous.number == FIRST {
            0
        } else {
            (previous.number + 1) % FIRST
        };
        self.number == expected
    }
}

struct Pending {
    message: FlexMessage,
    piece: Piece,
    since: u64,
}

/// Joins an alpha page that a long message spreads over later frames, keyed by capcode.
pub struct Reassembly {
    pending: Vec<Pending>,
    wait: u64,
    stale: bool,
}

impl Reassembly {
    pub fn new(wait: u64) -> Self {
        Self {
            pending: Vec::with_capacity(MAX_PENDING),
            wait,
            stale: false,
        }
    }

    pub fn accept(
        &mut self,
        message: FlexMessage,
        piece: Piece,
        now: u64,
        events: &mut Vec<DecoderEvent>,
    ) {
        let message = match self.take(message.address) {
            Some(earlier) if piece.follows(earlier.piece) => joined(earlier.message, message),
            earlier => {
                if let Some(earlier) = earlier {
                    events.push(finished(earlier.message, false));
                }
                let complete = message.complete && piece.number == FIRST;
                FlexMessage {
                    complete,
                    ..message
                }
            }
        };
        if !piece.continued {
            events.push(finished(message, true));
            return;
        }
        if self.pending.len() == MAX_PENDING {
            let oldest = self.pending.remove(0);
            events.push(finished(oldest.message, false));
        }
        self.pending.push(Pending {
            message,
            piece,
            since: now,
        });
    }

    /// Emits, as incomplete, every page whose next piece is overdue or whose channel was retuned.
    pub fn expire(&mut self, now: u64, events: &mut Vec<DecoderEvent>) {
        let stale = std::mem::take(&mut self.stale);
        let wait = self.wait;
        events.extend(
            self.pending
                .extract_if(.., |pending| {
                    stale || now.saturating_sub(pending.since) > wait
                })
                .map(|pending| finished(pending.message, false)),
        );
    }

    pub fn abandon(&mut self) {
        self.stale = true;
    }

    fn take(&mut self, address: u64) -> Option<Pending> {
        let index = self
            .pending
            .iter()
            .position(|pending| pending.message.address == address)?;
        Some(self.pending.remove(index))
    }
}

fn joined(mut earlier: FlexMessage, later: FlexMessage) -> FlexMessage {
    earlier.text.push_str(&later.text);
    earlier.errors_corrected += later.errors_corrected;
    earlier.complete &= later.complete;
    if earlier.text.len() > MAX_TEXT {
        let end = (0..=MAX_TEXT)
            .rev()
            .find(|&index| earlier.text.is_char_boundary(index))
            .unwrap_or(0);
        earlier.text.truncate(end);
        earlier.complete = false;
    }
    earlier
}

fn finished(mut message: FlexMessage, complete: bool) -> DecoderEvent {
    message.text = message.text.trim().to_owned();
    message.complete &= complete;
    DecoderEvent::Flex(message)
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::PagerPayload;

    use super::*;

    const WAIT: u64 = 100;

    fn piece(number: u8, continued: bool) -> Piece {
        Piece { number, continued }
    }

    fn message(address: u64, text: &str) -> FlexMessage {
        FlexMessage {
            address,
            payload: PagerPayload::Alpha,
            text: text.to_owned(),
            baud: 1_600,
            levels: 2,
            cycle: 0,
            frame: 0,
            phase: 'A',
            errors_corrected: 1,
            complete: true,
        }
    }

    fn pages(events: &[DecoderEvent]) -> Vec<(&str, bool)> {
        events
            .iter()
            .filter_map(|event| match event {
                DecoderEvent::Flex(page) => Some((page.text.as_str(), page.complete)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_header_names_the_piece() {
        assert_eq!(Piece::of(3 << 11 | 1 << 10), piece(3, true));
        assert_eq!(Piece::of(1 << 11), piece(1, false));
    }

    #[test]
    fn pieces_in_order_join_into_one_page_with_their_spaces() {
        let mut fragments = Reassembly::new(WAIT);
        let mut events = Vec::new();
        fragments.accept(
            message(7, "Trauma Alert # "),
            piece(3, true),
            0,
            &mut events,
        );
        fragments.accept(message(9, "other "), piece(3, false), 10, &mut events);
        fragments.accept(message(7, "of Patients "), piece(0, true), 20, &mut events);
        fragments.accept(
            message(7, "ETA: 4 MINS  "),
            piece(1, false),
            30,
            &mut events,
        );
        assert_eq!(
            pages(&events),
            [
                ("other", true),
                ("Trauma Alert # of Patients ETA: 4 MINS", true)
            ]
        );
        let DecoderEvent::Flex(joined) = &events[1] else {
            panic!("a flex page");
        };
        assert_eq!(joined.errors_corrected, 3);
    }

    #[test]
    fn a_continuation_without_its_start_is_incomplete() {
        let mut fragments = Reassembly::new(WAIT);
        let mut events = Vec::new();
        fragments.accept(message(7, "of Patients"), piece(0, false), 0, &mut events);
        assert_eq!(pages(&events), [("of Patients", false)]);
    }

    #[test]
    fn a_missing_middle_piece_marks_both_sides_incomplete() {
        let mut fragments = Reassembly::new(WAIT);
        let mut events = Vec::new();
        fragments.accept(message(7, "one "), piece(3, true), 0, &mut events);
        fragments.accept(message(7, "three"), piece(1, false), 10, &mut events);
        assert_eq!(pages(&events), [("one", false), ("three", false)]);
    }

    #[test]
    fn an_overdue_piece_surfaces_as_incomplete() {
        let mut fragments = Reassembly::new(WAIT);
        let mut events = Vec::new();
        fragments.accept(message(7, "Trauma "), piece(3, true), 0, &mut events);
        fragments.expire(WAIT, &mut events);
        assert!(events.is_empty());
        fragments.expire(WAIT + 1, &mut events);
        assert_eq!(pages(&events), [("Trauma", false)]);
    }

    #[test]
    fn a_retune_surfaces_every_waiting_piece() {
        let mut fragments = Reassembly::new(WAIT);
        let mut events = Vec::new();
        fragments.accept(message(7, "Trauma "), piece(3, true), 0, &mut events);
        fragments.abandon();
        fragments.expire(1, &mut events);
        assert_eq!(pages(&events), [("Trauma", false)]);
        fragments.accept(message(8, "next "), piece(3, true), 2, &mut events);
        fragments.expire(3, &mut events);
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn a_full_table_surfaces_its_oldest_page() {
        let mut fragments = Reassembly::new(WAIT);
        let mut events = Vec::new();
        for address in 0..=MAX_PENDING as u64 {
            fragments.accept(message(address, "x"), piece(3, true), 0, &mut events);
        }
        assert_eq!(events.len(), 1);
        let DecoderEvent::Flex(oldest) = &events[0] else {
            panic!("a flex page");
        };
        assert_eq!((oldest.address, oldest.complete), (0, false));
    }

    #[test]
    fn an_overlong_page_is_cut_and_marked_incomplete() {
        let mut fragments = Reassembly::new(WAIT);
        let mut events = Vec::new();
        let long = "x".repeat(MAX_TEXT);
        fragments.accept(message(7, &long), piece(3, true), 0, &mut events);
        fragments.accept(message(7, "tail"), piece(0, false), 1, &mut events);
        let DecoderEvent::Flex(page) = &events[0] else {
            panic!("a flex page");
        };
        assert_eq!((page.text.len(), page.complete), (MAX_TEXT, false));
    }
}
