use super::{
    super::{
        expect::Expected,
        message::{MAXGRID4, Standard},
        protocol::{BASEBAND_SYMBOL, MAX_SYMBOLS, MAX_TONES, SAMPLE_RATE},
        synth,
    },
    Engine, Found,
};

const REACH_S: f32 = 0.2;
const FREQUENCY_STEP_HZ: f32 = 0.5;
const FREQUENCY_STEPS: i32 = 3;
/// Least mean excess of the hypothesis tones over an even share of each data symbol's power.
pub(super) const MIN_SCORE: f32 = 0.55;
/// Least lead of the best hypothesis over the next best one.
pub(super) const MIN_MARGIN: f32 = 0.3;
/// Least lead of the best hypothesis over the decoys' mean, in decoy standard deviations.
pub(super) const MIN_SIGMAS: f32 = 6.0;
/// The same lead measured on the parity symbols alone.
pub(super) const MIN_PARITY_SIGMAS: f32 = 4.0;
/// Payloads no station is expected to send, scored like the hypotheses to show how well a wrong
/// message matches here. They keep the calls and change the grid, so a station sending something
/// outside the hypotheses matches them as well as it matches any hypothesis.
const DECOYS: usize = 64;
const PAYLOAD_MASK: u128 = (1 << 77) - 1;

type Powers = [[f32; MAX_TONES]; MAX_SYMBOLS];

/// How well a message's tones match the data symbols: over all of them, and over those carrying
/// only LDPC parity, which a message sharing calls or a report with the true one does not match.
#[derive(Clone, Copy)]
struct Match {
    all: f32,
    parity: f32,
}

impl Default for Match {
    fn default() -> Self {
        Self {
            all: f32::MIN,
            parity: f32::MIN,
        }
    }
}

struct Best {
    score: f32,
    runner_up: f32,
    sigmas: f32,
    parity_sigmas: f32,
    index: usize,
    offset_hz: f32,
    start: isize,
}

impl Best {
    fn convincing(&self) -> bool {
        self.score >= MIN_SCORE
            && self.score - self.runner_up >= MIN_MARGIN
            && self.sigmas >= MIN_SIGMAS
            && self.parity_sigmas >= MIN_PARITY_SIGMAS
    }
}

fn mix(index: u64) -> u64 {
    let mut z = index.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn decoys(seed: u128) -> Vec<u128> {
    (1..=DECOYS as u64)
        .map(|index| match Standard::parse(seed) {
            Some(message) => Standard {
                roger: false,
                extra: (mix(index) % u64::from(MAXGRID4)) as u32,
                ..message
            }
            .bits(),
            None => {
                let noise = (u128::from(mix(2 * index)) << 64) | u128::from(mix(2 * index + 1));
                (seed ^ noise) & PAYLOAD_MASK
            }
        })
        .collect()
}

fn sigmas_above(score: f32, reference: impl Iterator<Item = f32> + Clone) -> f32 {
    let count = reference.clone().count() as f32;
    if count < 2.0 {
        return 0.0;
    }
    let mean = reference.clone().sum::<f32>() / count;
    let variance = reference.map(|value| (value - mean).powi(2)).sum::<f32>() / (count - 1.0);
    if variance > 0.0 {
        (score - mean) / variance.sqrt()
    } else {
        0.0
    }
}

impl Engine {
    /// Tests each expectation's few likely messages against the residual left after the regular
    /// passes, as WSJT-X's a7 decoder does: the whole message is known, so no LDPC search runs
    /// and a signal too weak to decode blind can still be confirmed.
    pub(super) fn recall(
        &mut self,
        expected: &[Expected],
        found: &[Found],
        accept: &mut dyn FnMut(u128, bool) -> Option<String>,
    ) -> Vec<Found> {
        let mut recalled: Vec<Found> = Vec::new();
        for expectation in expected {
            if self.already_heard(expectation, found.iter().chain(&recalled)) {
                continue;
            }
            if let Some(result) = self.confirm(expectation, found, accept) {
                recalled.push(result);
            }
        }
        recalled
    }

    fn already_heard<'a>(
        &self,
        expectation: &Expected,
        mut found: impl Iterator<Item = &'a Found>,
    ) -> bool {
        let spacing = self.protocol.tone_spacing_hz();
        found.any(|result| {
            (result.frequency_hz - expectation.frequency_hz).abs() < spacing
                && (result.start_s - expectation.start_s).abs() < 1.0
        })
    }

    fn confirm(
        &mut self,
        expectation: &Expected,
        found: &[Found],
        accept: &mut dyn FnMut(u128, bool) -> Option<String>,
    ) -> Option<Found> {
        let protocol = self.protocol;
        let seed = expectation.payloads.first().copied()?;
        let tones: Vec<[u8; MAX_SYMBOLS]> = expectation
            .payloads
            .iter()
            .copied()
            .chain(decoys(seed))
            .map(|payload| protocol.tones_for(payload))
            .collect();
        let centre = self.downconvert(f64::from(expectation.frequency_hz));
        let best = self.best_hypothesis(expectation, centre, &tones)?;
        if !best.convincing() {
            return None;
        }
        let payload = expectation.payloads[best.index];
        if found.iter().any(|known| known.payload == payload) {
            return None;
        }
        let text = accept(payload, false)?;
        self.downconvert(f64::from(expectation.frequency_hz));
        self.rotate(best.offset_hz);
        let spectra = self.spectra(best.start);
        let tones = &tones[best.index];
        let snr_db = self.snr(&spectra, tones);
        let hard_errors = self.hard_errors(&spectra, payload);
        let lead = synth::lead_samples(protocol) as isize;
        let coarse = best.start * protocol.decimation() as isize - lead;
        let (start, drift_hz) = self.pinpoint(tones, coarse);
        Some(Found {
            payload,
            text,
            frequency_hz: (centre + f64::from(best.offset_hz) + drift_hz) as f32,
            start_s: start as f32 / SAMPLE_RATE,
            snr_db,
            hard_errors,
        })
    }

    fn best_hypothesis(
        &mut self,
        expectation: &Expected,
        centre: f64,
        tones: &[[u8; MAX_SYMBOLS]],
    ) -> Option<Best> {
        let protocol = self.protocol;
        let lead = synth::lead_samples(protocol) as f32;
        let decimation = protocol.decimation() as f32;
        let nominal = ((expectation.start_s * SAMPLE_RATE + lead) / decimation).round() as isize;
        let reach = (REACH_S * protocol.baseband_rate()) as isize;
        let near = (expectation.frequency_hz as f64 - centre) as f32;
        let (offset_hz, start) = self.sync_alignment(near, nominal, reach);
        self.rotate(offset_hz);
        let powers = self.data_powers(start);
        let scores: Vec<(Match, f32, isize)> = tones
            .iter()
            .map(|tones| (self.match_score(&powers, tones), offset_hz, start))
            .collect();
        let (hypotheses, decoys) = scores.split_at(expectation.payloads.len());
        let index = super::argmax_by(hypotheses, |score| score.0.all);
        let runner_up = hypotheses
            .iter()
            .enumerate()
            .filter(|&(other, _)| other != index)
            .chain(decoys.iter().enumerate())
            .fold(f32::MIN, |top, (_, score)| top.max(score.0.all));
        let (score, offset_hz, start) = *hypotheses.get(index)?;
        Some(Best {
            score: score.all,
            runner_up,
            sigmas: sigmas_above(score.all, decoys.iter().map(|decoy| decoy.0.all)),
            parity_sigmas: sigmas_above(score.parity, decoys.iter().map(|decoy| decoy.0.parity)),
            index,
            offset_hz,
            start,
        })
    }

    /// Where the Costas arrays sit near the expected time and frequency. Every hypothesis and
    /// decoy is then scored at this one alignment, so none gains by picking its own.
    fn sync_alignment(&mut self, near_hz: f32, nominal: isize, reach: isize) -> (f32, isize) {
        let mut best = (near_hz, nominal, f32::MIN);
        for step in -FREQUENCY_STEPS..=FREQUENCY_STEPS {
            let offset_hz = near_hz + step as f32 * FREQUENCY_STEP_HZ;
            let probe = self.probe(offset_hz, self.protocol.coherent_sync);
            for start in nominal - reach..=nominal + reach {
                let power = self.sync_score(start, &probe);
                if power > best.2 {
                    best = (offset_hz, start, power);
                }
            }
        }
        (best.0, best.1)
    }

    /// Each data symbol's tone powers as a share of that symbol's total, scaled so an even
    /// spread reads 1 on every tone.
    fn data_powers(&self, start: isize) -> Powers {
        let protocol = self.protocol;
        let mut powers = [[0.0; MAX_TONES]; MAX_SYMBOLS];
        for (symbol, row) in powers.iter_mut().enumerate().take(protocol.symbols) {
            if protocol.sync_tone(symbol).is_some() {
                continue;
            }
            let samples = self.symbol(start + (symbol * BASEBAND_SYMBOL) as isize);
            let mut total = 0.0;
            for (tone, power) in row.iter_mut().enumerate().take(protocol.tones) {
                *power = samples
                    .iter()
                    .zip(&self.twiddles[tone])
                    .map(|(sample, twiddle)| sample * twiddle)
                    .sum::<num_complex::Complex<f32>>()
                    .norm_sqr();
                total += *power;
            }
            if total > 0.0 {
                let scale = protocol.tones as f32 / total;
                row.iter_mut().for_each(|power| *power *= scale);
            }
        }
        powers
    }

    fn match_score(&self, powers: &Powers, tones: &[u8; MAX_SYMBOLS]) -> Match {
        let protocol = self.protocol;
        let (mut sum, mut count, mut parity, mut parity_count) = (0.0, 0, 0.0, 0);
        for (symbol, (row, &tone)) in powers.iter().zip(tones).enumerate().take(protocol.symbols) {
            if protocol.sync_tone(symbol).is_some() {
                continue;
            }
            let power = row[usize::from(tone)];
            if count * protocol.bits_per_symbol >= super::ldpc::K {
                parity += power;
                parity_count += 1;
            }
            sum += power;
            count += 1;
        }
        let mean = |sum: f32, count: usize| {
            if count == 0 {
                0.0
            } else {
                sum / count as f32 - 1.0
            }
        };
        Match {
            all: mean(sum, count),
            parity: mean(parity, parity_count),
        }
    }

    fn hard_errors(&self, spectra: &super::Spectra, payload: u128) -> u32 {
        let codeword = super::ldpc::encode(payload ^ self.protocol.scramble);
        let (llr, _) = self.llrs(spectra, 1, &self.combos[0]);
        llr.iter()
            .zip(codeword)
            .filter(|&(&value, bit)| (value > 0.0) != (bit == 1))
            .count() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::{
        super::super::{FT8, Protocol, expect::History, message},
        *,
    };

    struct Noise(u64);

    impl Noise {
        fn uniform(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((self.0 >> 11) as f64 / (1u64 << 53) as f64).max(1e-12)
        }

        fn gaussian(&mut self) -> f32 {
            let (u, v) = (self.uniform(), self.uniform());
            ((-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()) as f32
        }

        fn below(&mut self, limit: u64) -> u64 {
            (self.uniform() * limit as f64) as u64 % limit
        }
    }

    const CALLS: [&str; 8] = [
        "K1ABC", "W9XYZ", "DL1ABC", "JA1XYZ", "G4ABC", "VK2DEF", "OH8JK", "PA3EPP",
    ];
    const GRIDS: [&str; 4] = ["FN42", "EN37", "JO62", "PM95"];

    fn add(audio: &mut [f32], protocol: &Protocol, text: &str, hz: f32, start_s: f32, snr_db: f32) {
        let payload = message::pack(text).unwrap();
        let tones = protocol.tones_for(payload);
        let mut wave = Vec::new();
        synth::waveform(
            protocol,
            &tones[..protocol.symbols],
            f64::from(hz),
            &mut wave,
        );
        let amplitude = (2.0 * 10f32.powf(snr_db / 10.0) * 2_500.0 / 6_000.0).sqrt();
        let begin = (start_s * SAMPLE_RATE) as usize;
        for (sample, value) in audio[begin..].iter_mut().zip(&wave) {
            *sample += amplitude * value.re;
        }
    }

    fn message(noise: &mut Noise) -> String {
        let first = CALLS[noise.below(8) as usize];
        let mut second = CALLS[noise.below(8) as usize];
        while second == first {
            second = CALLS[noise.below(8) as usize];
        }
        match noise.below(4) {
            0 => format!("CQ {second} {}", GRIDS[noise.below(4) as usize]),
            1 => format!("{first} {second} {}", GRIDS[noise.below(4) as usize]),
            2 => format!("{first} {second} -{:02}", noise.below(25)),
            _ => format!("{first} {second} R-{:02}", noise.below(25)),
        }
    }

    fn expectation(protocol: &'static Protocol, text: &str, hz: f32, start_s: f32) -> Expected {
        let mut history = History::new(protocol);
        let heard = Found {
            payload: message::pack(text).unwrap(),
            text: text.to_owned(),
            frequency_hz: hz,
            start_s,
            snr_db: -20.0,
            hard_errors: 0,
        };
        history.remember(0, &[heard]);
        let slot = protocol.slot_samples as u64;
        history.expected(2 * slot, 100.0, 4_000.0).remove(0)
    }

    fn scores(
        engine: &mut Engine,
        audio: &[f32],
        expected: &Expected,
        sent: Option<u128>,
    ) -> (f32, f32, bool, bool) {
        engine.transform(audio);
        let centre = engine.downconvert(f64::from(expected.frequency_hz));
        let payloads: Vec<u128> = expected
            .payloads
            .iter()
            .copied()
            .chain(decoys(expected.payloads[0]))
            .collect();
        let tones: Vec<_> = payloads
            .iter()
            .map(|&p| engine.protocol.tones_for(p))
            .collect();
        let best = engine.best_hypothesis(expected, centre, &tones).unwrap();
        (
            best.parity_sigmas,
            best.score - best.runner_up,
            best.convincing(),
            Some(payloads[best.index]) == sent,
        )
    }

    fn quantiles(mut values: Vec<f32>) -> String {
        values.sort_unstable_by(f32::total_cmp);
        let at = |q: f32| values[((values.len() - 1) as f32 * q) as usize];
        format!(
            "p50 {:.3} p90 {:.3} p99 {:.3} max {:.3}",
            at(0.5),
            at(0.9),
            at(0.99),
            at(1.0)
        )
    }

    #[test]
    #[ignore = "threshold calibration; run in release"]
    fn calibrate_recall_thresholds() {
        let protocol: &'static Protocol = if std::env::var("PROTOCOL").is_ok_and(|p| p == "ft4") {
            &super::super::super::FT4
        } else {
            &FT8
        };
        let trials: usize = std::env::var("TRIALS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(200);
        let mut noise = Noise(7);
        let mut engine = Engine::new(protocol);
        let cases = std::env::var("CASES").unwrap_or_else(|_| {
            "noise,other -5,other -15,other -20,-20,-22,-24,-26,-28".to_owned()
        });
        for case in cases.split(',') {
            let (mut score, mut margin, mut pass, mut right) = (Vec::new(), Vec::new(), 0, 0);
            for _ in 0..trials {
                let mut audio: Vec<f32> = (0..protocol.slot_samples)
                    .map(|_| noise.gaussian())
                    .collect();
                let hz = 300.0 + noise.below(2_400) as f32;
                let spare = protocol.slot_samples - protocol.symbols * protocol.symbol_samples;
                let start_s = 0.1
                    + noise.below((spare as u64 * 100 / 12_000).saturating_sub(30)) as f32 / 100.0;
                let text = message(&mut noise);
                let expected = expectation(protocol, &text, hz, start_s);
                let sent = match case {
                    "noise" => None,
                    other if other.starts_with("other") => {
                        let mut other_text = message(&mut noise);
                        while other_text == text {
                            other_text = message(&mut noise);
                        }
                        let snr = other
                            .strip_prefix("other ")
                            .map_or(-5.0, |v| v.parse().unwrap());
                        add(&mut audio, protocol, &other_text, hz, start_s, snr);
                        let (s, m, decided, correct) =
                            scores(&mut engine, &audio, &expected, message::pack(&other_text));
                        if decided && !correct {
                            println!(
                                "  wrong: heard {text:?} now {other_text:?} sigmas {s:.1} margin {m:.2}"
                            );
                        }
                        message::pack(&other_text)
                    }
                    snr => {
                        add(
                            &mut audio,
                            protocol,
                            &text,
                            hz,
                            start_s,
                            snr.parse().unwrap(),
                        );
                        message::pack(&text)
                    }
                };
                let (s, m, decided, correct) = scores(&mut engine, &audio, &expected, sent);
                pass += usize::from(decided);
                right += usize::from(decided && correct);
                score.push(s);
                margin.push(m);
            }
            println!(
                "{case:>13}: pass {pass:>4}/{trials} right {right:>4} wrong {:>4}",
                pass - right
            );
            println!(
                "                | parity sigmas {} | margin {}",
                quantiles(score),
                quantiles(margin)
            );
        }
    }
}
