mod engine;
mod expect;
mod ldpc;
mod message;
mod protocol;
mod synth;

#[cfg(test)]
mod comparison;

pub(crate) use engine::Found;
pub(crate) use protocol::{FT4, FT8, Protocol};

use engine::{Engine, Search};
use expect::History;
use message::CallBook;

const PASSES: usize = 3;
const OSD_ORDER: usize = 3;

pub(crate) struct FtxDecoder {
    engine: Engine,
    book: CallBook,
    history: History,
}

impl FtxDecoder {
    pub(crate) fn new(protocol: &'static Protocol) -> Self {
        Self {
            engine: Engine::new(protocol),
            book: CallBook::default(),
            history: History::new(protocol),
        }
    }

    /// Decodes one window that starts `window_start` samples into the stream, so messages heard
    /// in earlier windows can be looked for again where they should recur.
    pub(crate) fn decode(
        &mut self,
        samples: &[f32],
        window_start: u64,
        low_hz: f32,
        high_hz: f32,
        max_candidates: usize,
    ) -> Vec<Found> {
        let search = Search {
            low_hz,
            high_hz,
            max_candidates,
            passes: PASSES,
            osd_order: OSD_ORDER,
        };
        let expected = self.history.expected(window_start, low_hz, high_hz);
        let book = &mut self.book;
        let found = self
            .engine
            .decode(samples, &search, &expected, &mut |payload, osd| {
                if osd && !message::is_plain_standard(payload) {
                    return None;
                }
                message::unpack(payload, book)
            });
        self.history.remember(window_start, &found);
        found
    }
}

#[cfg(any(test, feature = "synth"))]
pub(crate) fn pack(text: &str) -> Option<u128> {
    message::pack(text)
}

#[cfg(any(test, feature = "synth"))]
pub(crate) fn waveform(protocol: &Protocol, payload: u128, frequency_hz: f64) -> Vec<f32> {
    let tones = protocol.tones_for(payload);
    let mut wave = Vec::new();
    synth::waveform(
        protocol,
        &tones[..protocol.symbols],
        frequency_hz,
        &mut wave,
    );
    wave.iter().map(|sample| sample.re).collect()
}
