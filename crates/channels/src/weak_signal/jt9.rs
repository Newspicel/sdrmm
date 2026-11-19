//! JT9: one minute of 9-FSK at 1.736 baud, a sync tone and eight Gray-coded data tones carrying
//! the 72-bit JT message under the K=32 convolutional code.

use sdrmm_dsp::{
    coding::fec::fano::{Fano, FanoCode},
    math::stats::quantile,
};
use sdrmm_modem::k32;

use std::sync::LazyLock;

use sdrmm_decoder::registry::Native;
use sdrmm_wire::{ChannelDescriptor, EventKind, Verification};

use super::{
    WsjtParams,
    jt72::{self, WORDS, Words},
    minute::{self, MinuteDecoder},
    sync_tone::{Candidate, Heard, Layout, Lock, SyncTone},
};

const SYMBOLS: usize = 85;
const DATA_SYMBOLS: usize = 69;
const DATA_TONES: usize = 8;
const PAYLOAD_BITS: usize = 72;
const CODED_BITS: usize = 206;
const CHANNEL_BITS: usize = 3 * DATA_SYMBOLS;
const CODE: FanoCode<'static> = k32::code(PAYLOAD_BITS);
const LLR_LIMIT: f32 = 5.0;
const SATURATING_SHARE: f32 = 0.9;
const FANO_DELTA: f32 = 1.0;
const FANO_CYCLES: usize = 600_000;
const MIN_SYNC: f32 = 0.55;

const SYNC: [u8; SYMBOLS] = [
    1, 1, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0,
    0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1,
];

pub(crate) const LAYOUT: Layout = Layout {
    symbol_samples: 6_912.0,
    sync: &SYNC,
    tones: 1 + DATA_TONES,
    frame_fft: 13_824,
    baseband_bins: 2_500,
    min_score: 2.0,
};

fn interleaved() -> [usize; CODED_BITS] {
    let mut order = [0usize; CODED_BITS];
    k32::bit_reversal(&mut order);
    order
}

fn gray(value: u8) -> u8 {
    value ^ (value >> 1)
}

/// The 85 tones that send `words`, 0 the sync tone.
pub(crate) fn tones(words: &Words) -> [u8; SYMBOLS] {
    let mut coded = [0u8; CODED_BITS];
    let bits = words
        .iter()
        .flat_map(|&word| (0..6).rev().map(move |bit| word >> bit & 1 == 1));
    CODE.encode(bits, &mut coded);
    let mut channel = [0u8; CHANNEL_BITS];
    for (bit, position) in coded.iter().zip(interleaved()) {
        channel[position] = *bit;
    }
    let mut data = channel
        .as_chunks::<3>()
        .0
        .iter()
        .map(|bits| gray(bits[0] << 2 | bits[1] << 1 | bits[2]) + 1);
    SYNC.map(|flag| {
        if flag == 1 {
            0
        } else {
            data.next().unwrap_or(0)
        }
    })
}

pub(crate) struct Jt9Decoder {
    front: SyncTone,
    fano: Fano<'static>,
}

impl Jt9Decoder {
    pub(crate) fn new() -> Self {
        Self {
            front: SyncTone::new(&LAYOUT),
            fano: Fano::new(CODE),
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
            if heard.iter().any(|known| {
                (frequency - known.frequency_hz) > -spacing
                    && (frequency - known.frequency_hz) < (DATA_TONES as f32 + 0.5) * spacing
            }) {
                continue;
            }
            let Some((found, lock, words)) = self.attempt(&candidate) else {
                continue;
            };
            self.front.subtract(&lock, &tones(&words));
            if !heard.iter().any(|known| known.text == found.text) {
                heard.push(found);
            }
        }
        heard
    }

    fn attempt(&mut self, candidate: &Candidate) -> Option<(Heard, Lock, Words)> {
        let lock = self.front.lock(candidate);
        if lock.sync < MIN_SYNC {
            return None;
        }
        let spectra = self.front.spectra(&lock);
        let data: Vec<[f32; DATA_TONES]> = spectra
            .iter()
            .zip(SYNC)
            .filter(|&(_, flag)| flag == 0)
            .map(|(row, _)| std::array::from_fn(|tone| row[tone + 1]))
            .collect();
        let (signal, noise) = levels(&data);
        let words = self.fano_decode(&data, noise)?;
        let text = jt72::unpack(&words)?;
        let found = heard(&lock, &data, &words, text, signal, noise);
        Some((found, lock, words))
    }

    /// Fano-decodes bit metrics that grow with the power difference between the strongest tone
    /// saying 1 and the strongest saying 0, scaled so most symbols of a clean signal saturate:
    /// a symbol the signal is missing from, faded or not yet keyed, then counts for little.
    fn fano_decode(&mut self, data: &[[f32; DATA_TONES]], noise: f32) -> Option<Words> {
        let mut channel = [0.0f32; CHANNEL_BITS];
        for (symbol, powers) in data.iter().enumerate() {
            for bit in 0..3 {
                let (mut one, mut zero) = (0.0f32, 0.0f32);
                for (tone, &power) in powers.iter().enumerate() {
                    if inverse_gray(tone as u8) >> (2 - bit) & 1 == 1 {
                        one = one.max(power);
                    } else {
                        zero = zero.max(power);
                    }
                }
                channel[3 * symbol + bit] = (one - zero) / noise;
            }
        }
        let mut magnitudes = channel.map(f32::abs);
        let typical = quantile(&mut magnitudes, SATURATING_SHARE)
            .filter(|&value| value > 0.0)
            .unwrap_or(1.0);
        let order = interleaved();
        let metrics: [[f32; 2]; CODED_BITS] = std::array::from_fn(|bit| {
            CODE.metric((LLR_LIMIT * channel[order[bit]] / typical).clamp(-LLR_LIMIT, LLR_LIMIT))
        });
        if !self.fano.decode(&metrics, FANO_DELTA, FANO_CYCLES) {
            return None;
        }
        let mut words = [0u8; WORDS];
        for (index, bit) in self.fano.data().enumerate() {
            words[index / 6] |= u8::from(bit) << (5 - index % 6);
        }
        Some(words)
    }
}

fn inverse_gray(mut value: u8) -> u8 {
    let mut result = value;
    while value > 0 {
        value >>= 1;
        result ^= value;
    }
    result
}

/// Mean power of the strongest tone above the noise, and the mean of the others.
fn levels(data: &[[f32; DATA_TONES]]) -> (f32, f32) {
    let (mut peak, mut rest) = (0.0f32, 0.0f32);
    for powers in data {
        let top = powers.iter().copied().fold(0.0f32, f32::max);
        peak += top;
        rest += powers.iter().sum::<f32>() - top;
    }
    let noise = (rest / ((DATA_TONES - 1) * data.len()) as f32).max(f32::MIN_POSITIVE);
    let signal = (peak / data.len() as f32 - noise).max(noise * 1e-3);
    (signal, noise)
}

fn heard(
    lock: &Lock,
    data: &[[f32; DATA_TONES]],
    words: &Words,
    text: String,
    signal: f32,
    noise: f32,
) -> Heard {
    let sent = tones(words);
    let hard_errors = data
        .iter()
        .zip(sent.iter().zip(SYNC).filter(|&(_, flag)| flag == 0))
        .filter(|(powers, (tone, _))| {
            let strongest = powers
                .iter()
                .enumerate()
                .fold((0, f32::MIN), |a, (i, &p)| if p > a.1 { (i, p) } else { a })
                .0;
            strongest + 1 != usize::from(**tone)
        })
        .count() as u32;
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
    let tones = tones(&jt72::pack(text)?);
    Some(super::sync_tone::fsk(
        &tones,
        LAYOUT.symbol_samples,
        LAYOUT.tone_hz(),
        audio_hz,
    ))
}

pub(crate) static JT9_DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| {
    super::descriptor(
        "jt9",
        "JT9",
        "JT9 weak signal contacts",
        EventKind::named("jt9"),
        Verification::OnAir,
    )
});

pub(crate) fn jt9_decoder() -> Native<WsjtParams> {
    let decoder =
        Native::new::<super::Jt9Channel>(|params| super::search_band(super::wsjt_config(params)))
            .filter(|params| super::search_filter(super::wsjt_config(params)))
            .home(14_078_000.0)
            .form(super::wsjt_form)
            .kinds(|| vec![minute::kind("jt9", "JT9")]);
    cfg_select! {
        any(test, feature = "synth") => decoder.synth(JT9_SIGNALS),
        _ => decoder,
    }
}

impl MinuteDecoder for Jt9Decoder {
    fn decode(&mut self, samples: &[f32], low_hz: f32, high_hz: f32, limit: usize) -> Vec<Heard> {
        Jt9Decoder::decode(self, samples, low_hz, high_hz, limit)
    }
}

#[cfg(any(test, feature = "synth"))]
static JT9_SIGNALS: &[sdrmm_modem::synth::Signal] = &[sdrmm_modem::synth::Signal {
    id: "jt9",
    label: "JT9",
    rate_hz: sdrmm_modem::synth::AUDIO_HZ,
    render: sdrmm_modem::synth::Render::Fallible(|| {
        Ok(sdrmm_modem::synth::resample(
            &super::synth::jt9_slot("W1AW", "FN42", 1_500.0)?,
            12_000.0,
            sdrmm_modem::synth::AUDIO_HZ,
        ))
    }),
}];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_and_data_symbols_add_up() {
        assert_eq!(
            SYNC.iter().map(|&flag| usize::from(flag)).sum::<usize>(),
            16
        );
        assert_eq!(SYMBOLS - 16, DATA_SYMBOLS);
        assert_eq!(CODE.coded(), CODED_BITS);
    }

    #[test]
    fn gray_codes_invert() {
        for value in 0..8 {
            assert_eq!(inverse_gray(gray(value)), value);
        }
    }

    #[test]
    fn tones_mark_sync_with_tone_zero_only() {
        let tones = tones(&jt72::pack("CQ K1JT FN20").expect("packs"));
        for (tone, flag) in tones.iter().zip(SYNC) {
            assert_eq!(*tone == 0, flag == 1);
            assert!(*tone <= 8);
        }
    }
}
