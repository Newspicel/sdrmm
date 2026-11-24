//! JT65A: one minute of 65-FSK at 2.69 baud. A pseudo-random half of the 126 symbols send the
//! sync tone; the rest send the 72-bit JT message as a (63,12) Reed-Solomon codeword over GF(64),
//! interleaved and Gray-coded, two tone slots above it.

mod soft;

use std::sync::LazyLock;

use sdrmm_decoder::registry::Native;
use sdrmm_wire::{ChannelDescriptor, EventKind, Verification};

use super::{
    WsjtParams,
    jt72::{self, Words},
    minute::{self, MinuteDecoder},
    sync_tone::{Candidate, Heard, Layout, Lock, MAX_TONES, SyncTone},
};
use soft::{K, N, Received, SoftDecoder};

const SYMBOLS: usize = 126;
const DATA_OFFSET: usize = 2;
const TRIALS: usize = 1_000;
const BIRDIE_SYMBOLS: usize = 20;
const BIRDIE_TRIES: usize = 30;
const MIN_SYNC: f32 = 0.5;
const ROWS: usize = 7;
const COLUMNS: usize = 9;

const SYNC: [u8; SYMBOLS] = [
    1, 0, 0, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 1, 0, 0, 1, 0, 0, 0, 1,
    1, 1, 0, 0, 1, 1, 1, 1, 0, 1, 1, 0, 1, 1, 1, 1, 0, 0, 0, 1, 1, 0, 1, 0, 1, 0, 1, 1, 0, 0, 1, 1,
    0, 1, 0, 1, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 0, 0, 1, 0,
    1, 1, 0, 1, 0, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1,
];

pub(crate) const LAYOUT: Layout = Layout {
    symbol_samples: 4_096.0 * 12_000.0 / 11_025.0,
    sync: &SYNC,
    tones: MAX_TONES,
    frame_fft: 8_192,
    baseband_bins: 15_000,
    min_score: 1.5,
};

type Channel = [u8; N];

/// Interleaving writes the codeword down 7-row columns and reads it out along the rows.
fn interleave(codeword: &Channel) -> Channel {
    let mut out = [0u8; N];
    for row in 0..ROWS {
        for column in 0..COLUMNS {
            out[column + COLUMNS * row] = codeword[row + ROWS * column];
        }
    }
    out
}

fn deinterleave<T: Copy>(channel: &[T; N]) -> [T; N] {
    let mut out = *channel;
    for row in 0..ROWS {
        for column in 0..COLUMNS {
            out[row + ROWS * column] = channel[column + COLUMNS * row];
        }
    }
    out
}

fn gray(value: u8) -> u8 {
    value ^ (value >> 1)
}

fn inverse_gray(mut value: u8) -> u8 {
    let mut result = value;
    while value > 0 {
        value >>= 1;
        result ^= value;
    }
    result
}

/// The decoder-order codeword as data tone values, in channel order: the transmitted word is
/// the codeword reversed, then interleaved and Gray-coded.
fn channel_symbols(codeword: &Channel) -> Channel {
    let mut sent = *codeword;
    sent.reverse();
    interleave(&sent).map(gray)
}

/// The 126 tones that send `words`, 0 the sync tone.
pub(crate) fn tones(decoder: &SoftDecoder, words: &Words) -> [u8; SYMBOLS] {
    let mut data = [0u8; K];
    for (slot, &word) in data.iter_mut().zip(words.iter().rev()) {
        *slot = word;
    }
    sent_tones(&decoder.encode(&data))
}

fn sent_tones(codeword: &Channel) -> [u8; SYMBOLS] {
    let mut channel = channel_symbols(codeword).into_iter();
    SYNC.map(|flag| {
        if flag == 1 {
            0
        } else {
            channel.next().map_or(0, |value| value + DATA_OFFSET as u8)
        }
    })
}

pub(crate) struct Jt65Decoder {
    front: SyncTone,
    soft: SoftDecoder,
}

impl Jt65Decoder {
    pub(crate) fn new() -> Self {
        Self {
            front: SyncTone::new(&LAYOUT),
            soft: SoftDecoder::new(),
        }
    }

    pub(crate) fn decode(
        &mut self,
        samples: &[f32],
        low_hz: f32,
        high_hz: f32,
        max_candidates: usize,
    ) -> Vec<Heard> {
        self.front.load(samples, low_hz, high_hz);
        let mut heard: Vec<Heard> = Vec::new();
        let spacing = LAYOUT.tone_hz() as f32;
        for candidate in self.front.candidates(low_hz, high_hz, max_candidates) {
            let frequency = candidate.frequency_hz as f32;
            if heard
                .iter()
                .any(|known| (frequency - known.frequency_hz).abs() < spacing)
            {
                continue;
            }
            let Some((found, lock, tones)) = self.attempt(&candidate) else {
                continue;
            };
            self.front.subtract(&lock, &tones);
            if !heard.iter().any(|known| known.text == found.text) {
                heard.push(found);
            }
        }
        heard
    }

    fn attempt(&mut self, candidate: &Candidate) -> Option<(Heard, Lock, [u8; SYMBOLS])> {
        let lock = self.front.lock(candidate);
        if lock.sync < MIN_SYNC {
            return None;
        }
        let spectra = self.front.spectra(&lock);
        let mut data = data_powers(&spectra);
        normalise(&mut data);
        let reference = data;
        let ranked = rank_without_birdies(&mut data)?;
        let received = Received::rank(&to_decoder_order(&ranked));
        let decoded = self.soft.decode(&received, TRIALS, &mut |codeword| {
            mean_power(&reference, &channel_symbols(codeword))
        })?;
        let mut words = [0u8; K];
        for (word, &symbol) in words.iter_mut().zip(decoded.codeword[..K].iter().rev()) {
            *word = symbol;
        }
        let text = jt72::unpack(&words)?;
        let found = heard(&lock, &reference, &decoded.codeword, decoded.hard, text);
        Some((found, lock, sent_tones(&decoded.codeword)))
    }
}

fn data_powers(spectra: &[[f32; MAX_TONES]]) -> [[f32; 64]; N] {
    let mut data = [[0.0f32; 64]; N];
    for (slot, (row, _)) in data
        .iter_mut()
        .zip(spectra.iter().zip(SYNC).filter(|&(_, flag)| flag == 0))
    {
        slot.copy_from_slice(&row[DATA_OFFSET..DATA_OFFSET + 64]);
    }
    data
}

fn normalise(data: &mut [[f32; 64]; N]) {
    let mut all: Vec<f32> = data.iter().flatten().copied().collect();
    let base = sdrmm_dsp::math::stats::median(&mut all)
        .filter(|&level| level > 0.0)
        .unwrap_or(1.0);
    for value in data.iter_mut().flatten() {
        *value /= base;
    }
}

/// Flattens tones that win too many symbols to be data, a carrier or birdie in the passband.
fn rank_without_birdies(data: &mut [[f32; 64]; N]) -> Option<[[f32; 64]; N]> {
    for _ in 0..BIRDIE_TRIES {
        let mut counts = [0usize; 64];
        for row in data.iter() {
            counts[strongest(row)] += 1;
        }
        let (tone, &count) = counts
            .iter()
            .enumerate()
            .max_by_key(|&(_, count)| count)
            .unwrap_or((0, &0));
        if count < BIRDIE_SYMBOLS {
            return Some(*data);
        }
        let mut all: Vec<f32> = data.iter().flatten().copied().collect();
        let base = sdrmm_dsp::math::stats::median(&mut all).unwrap_or(1.0);
        for row in data.iter_mut() {
            row[tone] = base;
        }
    }
    None
}

fn strongest(row: &[f32; 64]) -> usize {
    row.iter()
        .enumerate()
        .fold((0, f32::MIN), |a, (i, &p)| if p > a.1 { (i, p) } else { a })
        .0
}

/// Channel-order tone powers, indexed by the tone's Gray-coded value, as decoder-order
/// symbols indexed by the symbol value.
fn to_decoder_order(data: &[[f32; 64]; N]) -> [[f32; 64]; N] {
    let by_value: [[f32; 64]; N] = data.map(|row| {
        let mut out = [0.0f32; 64];
        for (tone, &power) in row.iter().enumerate() {
            out[usize::from(inverse_gray(tone as u8))] = power;
        }
        out
    });
    let mut ordered = deinterleave(&by_value);
    ordered.reverse();
    ordered
}

fn mean_power(data: &[[f32; 64]; N], channel: &Channel) -> f32 {
    data.iter()
        .zip(channel)
        .map(|(row, &value)| row[usize::from(value)])
        .sum::<f32>()
        / N as f32
}

fn heard(
    lock: &Lock,
    data: &[[f32; 64]; N],
    codeword: &Channel,
    hard_errors: u32,
    text: String,
) -> Heard {
    let channel = channel_symbols(codeword);
    let (mut signal, mut rest) = (0.0f32, 0.0f32);
    for (row, &value) in data.iter().zip(&channel) {
        signal += row[usize::from(value)];
        rest += row.iter().sum::<f32>() - row[usize::from(value)];
    }
    let noise = (rest / (63.0 * N as f32)).max(f32::MIN_POSITIVE);
    let signal = (signal / N as f32 - noise).max(noise * 1e-3);
    Heard {
        text,
        frequency_hz: lock.frequency_hz as f32,
        start_s: lock.start_s as f32,
        snr_db: 10.0 * (signal / noise).log10()
            + 10.0 * (LAYOUT.tone_hz() as f32 / 2_500.0).log10(),
        hard_errors,
    }
}

#[cfg(any(test, feature = "synth"))]
pub(crate) fn waveform(text: &str, audio_hz: f32) -> Option<Vec<f32>> {
    let tones = tones(&SoftDecoder::new(), &jt72::pack(text)?);
    Some(super::sync_tone::fsk(
        &tones,
        LAYOUT.symbol_samples,
        LAYOUT.tone_hz(),
        audio_hz,
    ))
}

pub(crate) static JT65_DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| {
    super::descriptor(
        "jt65",
        "JT65",
        "JT65 weak signal contacts",
        EventKind::named("jt65"),
        Verification::OnAir,
    )
});

pub(crate) fn jt65_decoder() -> Native<WsjtParams> {
    let decoder =
        Native::new::<super::Jt65Channel>(|params| super::search_band(super::wsjt_config(params)))
            .filter(|params| super::search_filter(super::wsjt_config(params)))
            .home(14_076_000.0)
            .form(super::wsjt_form)
            .kinds(|| vec![minute::kind("jt65", "JT65")]);
    cfg_select! {
        any(test, feature = "synth") => decoder.synth(JT65_SIGNALS),
        _ => decoder,
    }
}

impl MinuteDecoder for Jt65Decoder {
    fn decode(&mut self, samples: &[f32], low_hz: f32, high_hz: f32, limit: usize) -> Vec<Heard> {
        Self::decode(self, samples, low_hz, high_hz, limit)
    }
}

#[cfg(any(test, feature = "synth"))]
static JT65_SIGNALS: &[sdrmm_modem::synth::Signal] = &[sdrmm_modem::synth::Signal {
    id: "jt65",
    label: "JT65",
    rate_hz: sdrmm_modem::synth::AUDIO_HZ,
    render: sdrmm_modem::synth::Render::Fallible(|| {
        Ok(sdrmm_modem::synth::resample(
            &super::synth::jt65_slot("W1AW", "FN42", 1_500.0)?,
            12_000.0,
            sdrmm_modem::synth::AUDIO_HZ,
        ))
    }),
}];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_the_symbols_are_sync() {
        assert_eq!(
            SYNC.iter().map(|&flag| usize::from(flag)).sum::<usize>(),
            63
        );
    }

    #[test]
    fn interleaving_inverts() {
        let codeword: Channel = std::array::from_fn(|i| i as u8);
        assert_eq!(deinterleave(&interleave(&codeword)), codeword);
        assert_eq!(interleave(&codeword)[1], 7);
    }

    #[test]
    fn tones_mark_sync_with_tone_zero_only() {
        let decoder = SoftDecoder::new();
        let words = jt72::pack("CQ K1JT FN20").expect("packs");
        let tones = tones(&decoder, &words);
        assert!(
            tones
                .iter()
                .zip(SYNC)
                .all(|(&tone, flag)| (tone == 0) == (flag == 1))
        );
    }

    #[test]
    fn decoder_order_undoes_the_channel_order() {
        let decoder = SoftDecoder::new();
        let codeword = decoder.encode(&[5, 4, 3, 2, 1, 0, 63, 62, 61, 60, 59, 58]);
        let channel = channel_symbols(&codeword);
        let mut powers = [[0.0f32; 64]; N];
        for (row, &value) in powers.iter_mut().zip(&channel) {
            row[usize::from(value)] = 1.0;
        }
        let ordered = to_decoder_order(&powers);
        for (row, &value) in ordered.iter().zip(&codeword) {
            assert_eq!(strongest(row), usize::from(value));
        }
    }
}
