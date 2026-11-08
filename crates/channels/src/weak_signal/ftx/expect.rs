use super::{
    Found, Protocol,
    message::{EXTRA_73, EXTRA_RR73, EXTRA_RRR, Standard, report_extra},
    protocol::SAMPLE_RATE,
};

const REPORTS: std::ops::RangeInclusive<i32> = -30..=30;
const MAX_PARTNERS: usize = 3;
const SAME_SIGNAL_HZ: f32 = 3.0;

/// A message heard earlier, placed on the channel's own sample clock.
#[derive(Clone, Copy)]
struct Heard {
    payload: u128,
    frequency_hz: f32,
    start: i64,
}

/// Where a station heard one period ago should be sending again, and what it may send.
#[derive(Clone, Debug)]
pub(crate) struct Expected {
    pub(crate) frequency_hz: f32,
    pub(crate) start_s: f32,
    pub(crate) payloads: Vec<u128>,
}

/// What the decoder heard in recent windows, the source of WSJT-X style a7 hypotheses: a
/// station keeps its audio frequency and its timing, so its next message is one of a few
/// that follow from its last one.
pub(crate) struct History {
    protocol: &'static Protocol,
    heard: Vec<Heard>,
}

impl History {
    pub(crate) fn new(protocol: &'static Protocol) -> Self {
        Self {
            protocol,
            heard: Vec::new(),
        }
    }

    fn slot(&self) -> i64 {
        self.protocol.slot_samples as i64
    }

    fn transmission(&self) -> i64 {
        (self.protocol.symbols * self.protocol.symbol_samples) as i64
    }

    pub(crate) fn remember(&mut self, window_start: u64, found: &[Found]) {
        let oldest = window_start as i64 - 3 * self.slot();
        self.heard.retain(|heard| heard.start >= oldest);
        for result in found {
            let start = window_start as i64 + (result.start_s * SAMPLE_RATE).round() as i64;
            let known = self.heard.iter().any(|heard| {
                heard.payload == result.payload && (heard.start - start).abs() < SAMPLE_RATE as i64
            });
            if !known {
                self.heard.push(Heard {
                    payload: result.payload,
                    frequency_hz: result.frequency_hz,
                    start,
                });
            }
        }
    }

    pub(crate) fn expected(&self, window_start: u64, low_hz: f32, high_hz: f32) -> Vec<Expected> {
        let window_start = window_start as i64;
        let mut expected: Vec<Expected> = Vec::new();
        for heard in &self.heard {
            let start = heard.start + 2 * self.slot();
            let local = start - window_start;
            if local < 0
                || local + self.transmission() > self.slot()
                || !(low_hz..=high_hz).contains(&heard.frequency_hz)
            {
                continue;
            }
            let duplicate = expected.iter().any(|known| {
                (known.frequency_hz - heard.frequency_hz).abs() < SAME_SIGNAL_HZ
                    && ((known.start_s * SAMPLE_RATE) as i64 - local).abs() < SAMPLE_RATE as i64
            });
            if duplicate {
                continue;
            }
            expected.push(Expected {
                frequency_hz: heard.frequency_hz,
                start_s: local as f32 / SAMPLE_RATE,
                payloads: self.hypotheses(heard.payload, start),
            });
        }
        expected
    }

    fn hypotheses(&self, payload: u128, start: i64) -> Vec<u128> {
        let mut payloads = vec![payload];
        let Some(last) = Standard::parse(payload) else {
            return payloads;
        };
        if !last.opens_with_token() {
            payloads.extend(replies(last.first, last.second, last.i3));
        }
        for partner in self.callers(last.second, start - self.slot()) {
            payloads.extend(replies(partner, last.second, last.i3));
        }
        payloads.sort_unstable();
        payloads.dedup();
        payloads
    }

    /// Stations heard calling `station` in the period before `around`.
    fn callers(&self, station: u32, around: i64) -> Vec<u32> {
        let mut callers: Vec<u32> = self
            .heard
            .iter()
            .filter(|heard| (heard.start - around).abs() < 2 * SAMPLE_RATE as i64)
            .filter_map(|heard| Standard::parse(heard.payload))
            .filter(|message| message.first == station)
            .map(|message| message.second)
            .collect();
        callers.sort_unstable();
        callers.dedup();
        callers.truncate(MAX_PARTNERS);
        callers
    }
}

/// Every message `sender` can send next to `partner`: a report, a roger report, or a sign-off.
fn replies(partner: u32, sender: u32, i3: u8) -> impl Iterator<Item = u128> {
    let message = move |roger, extra| {
        Standard {
            first: partner,
            second: sender,
            roger,
            extra,
            i3,
        }
        .bits()
    };
    [EXTRA_RRR, EXTRA_RR73, EXTRA_73]
        .into_iter()
        .map(move |extra| message(false, extra))
        .chain(REPORTS.flat_map(move |snr| {
            [false, true]
                .into_iter()
                .map(move |roger| message(roger, report_extra(snr)))
        }))
}

#[cfg(test)]
mod tests {
    use super::{
        super::{FT8, message},
        *,
    };

    fn found(text: &str, frequency_hz: f32, start_s: f32) -> Found {
        Found {
            payload: message::pack(text).unwrap(),
            text: text.to_owned(),
            frequency_hz,
            start_s,
            snr_db: -10.0,
            hard_errors: 0,
        }
    }

    fn texts(payloads: &[u128]) -> Vec<String> {
        let mut book = message::CallBook::default();
        payloads
            .iter()
            .filter_map(|&payload| message::unpack(payload, &mut book))
            .collect()
    }

    #[test]
    fn a_station_is_expected_one_period_later_with_its_next_messages() {
        let mut history = History::new(&FT8);
        history.remember(0, &[found("K1ABC W9XYZ EN37", 1_200.0, 0.6)]);
        let slot = FT8.slot_samples as u64;
        let expected = history.expected(2 * slot, 200.0, 3_000.0);
        assert_eq!(expected.len(), 1);
        assert!((expected[0].start_s - 0.6).abs() < 1e-3);
        let texts = texts(&expected[0].payloads);
        for wanted in [
            "K1ABC W9XYZ EN37",
            "K1ABC W9XYZ R-12",
            "K1ABC W9XYZ -07",
            "K1ABC W9XYZ RR73",
        ] {
            assert!(
                texts.iter().any(|text| text == wanted),
                "{wanted} in {texts:?}"
            );
        }
        assert!(
            history
                .expected(2 * slot + 5 * 12_000, 200.0, 3_000.0)
                .is_empty()
        );
        assert!(history.expected(2 * slot, 1_500.0, 3_000.0).is_empty());
    }

    #[test]
    fn a_cq_is_expected_to_repeat_or_answer_its_callers() {
        let mut history = History::new(&FT8);
        let slot = FT8.slot_samples as u64;
        history.remember(0, &[found("CQ W9XYZ EN37", 1_200.0, 0.6)]);
        history.remember(slot, &[found("W9XYZ DL1ABC JO62", 800.0, 0.4)]);
        let expected = history.expected(2 * slot, 200.0, 3_000.0);
        let cq = expected
            .iter()
            .find(|expected| expected.frequency_hz == 1_200.0)
            .unwrap();
        let texts = texts(&cq.payloads);
        assert!(texts.iter().any(|text| text == "CQ W9XYZ EN37"));
        assert!(texts.iter().any(|text| text == "DL1ABC W9XYZ -15"));
        assert!(!texts.iter().any(|text| text.starts_with("W9XYZ")));
    }
}
