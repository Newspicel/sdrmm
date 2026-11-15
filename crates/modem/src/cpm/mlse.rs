mod response;

pub use self::response::SymbolResponse;

use super::params::CpmParams;
use crate::soft::SoftBit;

const MAX_STATES: usize = 4_096;

const WINDOW_SYMBOLS: usize = 64;

const GAIN_SYMBOLS: f32 = 60.0;

const MAX_BITS: usize = 8;

const TAIL_SYMBOLS_PER_TAP: usize = 4;

const MIN_TAIL_SYMBOLS: usize = 8;

pub struct MlseDetector {
    levels: Vec<f32>,
    m: usize,
    bits_per_symbol: u32,
    soft_scale: f32,
    response: SymbolResponse,
    states: usize,
    branch: Vec<f32>,
    decides: Vec<u8>,
    tail: usize,
    pending: Vec<f32>,
    pending_gain: Vec<f32>,
    mean_abs_y: f32,
    alpha: Vec<f32>,
    beta: Vec<f32>,
    beta_next: Vec<f32>,
    marginal: Vec<f32>,
    decisions: Vec<(u8, [f32; MAX_BITS])>,
}

impl MlseDetector {
    #[must_use]
    pub fn new(params: &CpmParams, receive_filter: &[f32]) -> Self {
        Self::with_response(params, SymbolResponse::of(params, receive_filter))
    }

    #[must_use]
    pub fn with_response(params: &CpmParams, response: SymbolResponse) -> Self {
        let mapping = params.mapping();
        let m = mapping.m();
        let taps = response.taps().len();
        let states = m
            .checked_pow(taps as u32 - 1)
            .filter(|&s| s <= MAX_STATES)
            .unwrap_or_else(|| {
                panic!(
                    "a trellis of {m}^{} states is past the {MAX_STATES} cap: the response \
                     truncated to {taps} taps",
                    taps - 1
                )
            });
        let mut detector = Self {
            levels: mapping.levels().to_vec(),
            m,
            bits_per_symbol: mapping.bits_per_symbol(),
            soft_scale: 0.5
                / (mapping.min_spacing()
                    * mapping.min_spacing()
                    * response.taps().iter().map(|&c| c * c).sum::<f32>()),
            states,
            branch: vec![0.0; states * m],
            decides: vec![0; states * m],
            tail: training_tail(taps),
            pending: Vec::with_capacity(WINDOW_SYMBOLS),
            pending_gain: Vec::with_capacity(WINDOW_SYMBOLS),
            mean_abs_y: response.mean_abs(),
            alpha: vec![0.0; (WINDOW_SYMBOLS + 1) * states],
            beta: vec![0.0; states],
            beta_next: vec![0.0; states],
            marginal: vec![0.0; m],
            decisions: Vec::with_capacity(WINDOW_SYMBOLS),
            response,
        };
        detector.fill_tables();
        detector
    }

    #[must_use]
    pub fn response(&self) -> &SymbolResponse {
        &self.response
    }

    #[must_use]
    pub fn states(&self) -> usize {
        self.states
    }

    pub fn process(&mut self, soft: &[f32], symbols: &mut Vec<u8>, bits: &mut Vec<SoftBit>) {
        for &y in soft {
            self.push(y);
            if self.pending.len() == WINDOW_SYMBOLS {
                let emit = WINDOW_SYMBOLS - self.tail;
                self.run_window(WINDOW_SYMBOLS, emit);
                self.emit(symbols, bits);
                self.retain(emit);
            }
        }
    }

    pub fn flush(&mut self, symbols: &mut Vec<u8>, bits: &mut Vec<SoftBit>) {
        let held = self.pending.len();
        if held == 0 {
            return;
        }
        self.run_window(held, held);
        self.emit(symbols, bits);
        self.retain(held);
    }

    pub fn reset(&mut self) {
        self.pending.clear();
        self.pending_gain.clear();
        self.decisions.clear();
        self.mean_abs_y = self.response.mean_abs();
        self.alpha.fill(0.0);
    }

    fn fill_tables(&mut self) {
        let lead = self.response.lead();
        let digit = self.m.pow(lead.saturating_sub(1) as u32);
        for state in 0..self.states {
            for sym in 0..self.m {
                let mut sum = self.response.taps()[0] * self.levels[sym];
                let mut rest = state;
                for &tap in &self.response.taps()[1..] {
                    sum += tap * self.levels[rest % self.m];
                    rest /= self.m;
                }
                self.branch[state * self.m + sym] = sum;
                self.decides[state * self.m + sym] = if lead == 0 {
                    sym as u8
                } else {
                    (state / digit % self.m) as u8
                };
            }
        }
    }

    fn push(&mut self, y: f32) {
        self.mean_abs_y += (y.abs() - self.mean_abs_y) / GAIN_SYMBOLS;
        self.pending.push(y);
        self.pending_gain
            .push(self.mean_abs_y / self.response.mean_abs());
    }

    fn run_window(&mut self, len: usize, emit: usize) {
        let Self {
            m,
            states,
            branch,
            decides,
            pending,
            pending_gain,
            alpha,
            beta,
            beta_next,
            marginal,
            decisions,
            bits_per_symbol,
            soft_scale,
            ..
        } = self;
        let (m, states) = (*m, *states);

        for k in 0..len {
            let (done, ahead) = alpha.split_at_mut((k + 1) * states);
            let prev = &done[k * states..];
            let next = &mut ahead[..states];
            next.fill(f32::INFINITY);
            let (y, gain) = (pending[k], pending_gain[k]);
            for (state, &from) in prev.iter().enumerate() {
                for sym in 0..m {
                    let error = y - gain * branch[state * m + sym];
                    let metric = from + error * error;
                    let to = (state * m + sym) % states;
                    if metric < next[to] {
                        next[to] = metric;
                    }
                }
            }
            let floor = next.iter().copied().fold(f32::INFINITY, f32::min);
            for metric in next.iter_mut() {
                *metric -= floor;
            }
        }

        beta_next.fill(0.0);
        decisions.clear();
        for k in (0..len).rev() {
            let (y, gain) = (pending[k], pending_gain[k]);
            beta.fill(f32::INFINITY);
            marginal.fill(f32::INFINITY);
            for state in 0..states {
                let behind = alpha[k * states + state];
                for sym in 0..m {
                    let error = y - gain * branch[state * m + sym];
                    let onward = error * error + beta_next[(state * m + sym) % states];
                    if onward < beta[state] {
                        beta[state] = onward;
                    }
                    let decided = decides[state * m + sym] as usize;
                    let total = behind + onward;
                    if total < marginal[decided] {
                        marginal[decided] = total;
                    }
                }
            }
            if k < emit {
                decisions.push(decide(marginal, m, *bits_per_symbol, *soft_scale));
            }
            std::mem::swap(beta, beta_next);
        }
    }

    fn emit(&mut self, symbols: &mut Vec<u8>, bits: &mut Vec<SoftBit>) {
        for &(sym, soft) in self.decisions.iter().rev() {
            symbols.push(sym);
            bits.extend(
                soft[..self.bits_per_symbol as usize]
                    .iter()
                    .map(|&b| SoftBit(b)),
            );
        }
    }

    fn retain(&mut self, emitted: usize) {
        let keep = self.pending.len() - emitted;
        self.pending.copy_within(emitted.., 0);
        self.pending.truncate(keep);
        self.pending_gain.copy_within(emitted.., 0);
        self.pending_gain.truncate(keep);
        self.alpha
            .copy_within(emitted * self.states..(emitted + 1) * self.states, 0);
    }
}

fn training_tail(taps: usize) -> usize {
    (TAIL_SYMBOLS_PER_TAP * taps.saturating_sub(1)).clamp(MIN_TAIL_SYMBOLS, WINDOW_SYMBOLS / 2)
}

fn decide(
    marginal: &[f32],
    m: usize,
    bits_per_symbol: u32,
    soft_scale: f32,
) -> (u8, [f32; MAX_BITS]) {
    let mut best = 0usize;
    for sym in 1..m {
        if marginal[sym] < marginal[best] {
            best = sym;
        }
    }
    let mut soft = [0.0f32; MAX_BITS];
    for k in 0..bits_per_symbol {
        let (mut zero, mut one) = (f32::INFINITY, f32::INFINITY);
        for (sym, &metric) in marginal.iter().enumerate().take(m) {
            if sym >> k & 1 == 0 {
                zero = zero.min(metric);
            } else {
                one = one.min(metric);
            }
        }
        soft[(bits_per_symbol - 1 - k) as usize] = ((zero - one) * soft_scale).clamp(-1.0, 1.0);
    }
    (best as u8, soft)
}

#[cfg(test)]
mod tests {
    use crate::testutil::draws;
    use sdrmm_modem_test_support::ber::rng::Rng;

    use super::{super::mapping::Mapping, *};
    use crate::pulse::{self, Norm};

    const SPS: f64 = 10.0;

    fn gmsk(bt: f64, span: usize) -> CpmParams {
        CpmParams::from_h(
            Mapping::natural(2),
            0.5,
            pulse::gaussian_freq(SPS, bt, span, Norm::Area),
            SPS,
        )
    }

    fn gmsk_rx(bt: f64, span: usize) -> Vec<f32> {
        pulse::gaussian_freq(SPS, bt, span, Norm::Area)
    }

    fn model_stream(response: &SymbolResponse, mapping: &Mapping, sent: &[u8]) -> Vec<f32> {
        let taps = response.taps();
        (0..sent.len())
            .map(|k| {
                let newest = k + response.lead();
                taps.iter()
                    .enumerate()
                    .map(|(t, &tap)| {
                        newest
                            .checked_sub(t)
                            .and_then(|i| sent.get(i))
                            .map_or(0.0, |&s| tap * mapping.level(s))
                    })
                    .sum()
            })
            .collect()
    }

    fn detect(detector: &mut MlseDetector, soft: &[f32]) -> Vec<u8> {
        let mut symbols = Vec::new();
        let mut bits = Vec::new();
        detector.process(soft, &mut symbols, &mut bits);
        detector.flush(&mut symbols, &mut bits);
        symbols
    }

    #[test]
    fn a_nyquist_cascade_has_no_isi_to_remove() {
        let params = CpmParams::from_deviation(
            Mapping::new(vec![1.0, 3.0, -1.0, -3.0]),
            1_944.0,
            4_800.0,
            pulse::root_raised_cosine(SPS, 0.2, 8, Norm::Area),
            SPS,
        );
        let rx = pulse::root_raised_cosine(SPS, 0.2, 8, Norm::Area);
        let response = SymbolResponse::of(&params, &rx);
        assert!(
            response.is_isi_free(),
            "RRC ⊗ RRC kept {} taps: {:?}",
            response.taps().len(),
            response.taps()
        );
        assert_eq!(MlseDetector::new(&params, &rx).states(), 1);

        let sent = draws(400, 4, 0x4e11);
        let observed = model_stream(&response, params.mapping(), &sent);
        let got = detect(&mut MlseDetector::new(&params, &rx), &observed);
        assert_eq!(
            got, sent,
            "an ISI-free trellis decided a different sequence"
        );
    }
    #[test]
    fn noiseless_partial_response_decodes_without_error() {
        let params = gmsk(0.3, 4);
        let rx = gmsk_rx(0.3, 4);
        let response = SymbolResponse::of(&params, &rx);
        let sent = draws(600, 2, 0x51a7);
        let observed = model_stream(&response, params.mapping(), &sent);
        let mut detector = MlseDetector::new(&params, &rx);
        let got = detect(&mut detector, &observed);
        assert_eq!(got.len(), sent.len(), "one decision per observation");
        let span = response.lead()..sent.len() - response.lead();
        let errors = span.clone().filter(|&i| got[i] != sent[i]).count();
        assert_eq!(errors, 0, "sequence errors on a noiseless partial response");
    }

    #[test]
    fn decisions_match_exhaustive_maximum_likelihood() {
        const LEN: usize = 14;
        let params = gmsk(0.3, 4);
        let rx = gmsk_rx(0.3, 4);
        let response = SymbolResponse::of(&params, &rx);
        let mapping = params.mapping();
        let mut rng = Rng::new(0x3b1e);
        for trial in 0..8u64 {
            let sent = draws(LEN, 2, 0x100 + trial);
            let clean = model_stream(&response, mapping, &sent);
            let observed: Vec<f32> = clean
                .iter()
                .map(|&y| y + 0.25 * (rng.uniform() as f32 * 2.0 - 1.0))
                .collect();

            let mut best = (f32::INFINITY, Vec::new());
            for code in 0u32..1 << LEN {
                let candidate: Vec<u8> = (0..LEN).map(|i| (code >> i & 1) as u8).collect();
                let distance: f32 = model_stream(&response, mapping, &candidate)
                    .iter()
                    .zip(&observed)
                    .map(|(&p, &y)| (p - y) * (p - y))
                    .sum();
                if distance < best.0 {
                    best = (distance, candidate);
                }
            }

            let mut detector = MlseDetector::new(&params, &rx);
            let got = detect(&mut detector, &observed);
            let span = response.lead()..LEN - response.lead();
            assert_eq!(
                got[span.clone()],
                best.1[span],
                "trial {trial}: the sequence detector disagreed with exhaustive ML"
            );
        }
    }

    #[test]
    fn block_splits_do_not_change_the_decisions() {
        let params = gmsk(0.3, 4);
        let rx = gmsk_rx(0.3, 4);
        let response = SymbolResponse::of(&params, &rx);
        let observed = model_stream(&response, params.mapping(), &draws(500, 2, 0x9c4));

        let mut whole = MlseDetector::new(&params, &rx);
        let expected = detect(&mut whole, &observed);

        let mut split = MlseDetector::new(&params, &rx);
        let (mut got, mut bits) = (Vec::new(), Vec::new());
        let mut pos = 0;
        for len in [37usize, 1, 128, 5, 211].iter().cycle() {
            if pos >= observed.len() {
                break;
            }
            let end = (pos + len).min(observed.len());
            split.process(&observed[pos..end], &mut got, &mut bits);
            pos = end;
        }
        split.flush(&mut got, &mut bits);
        assert_eq!(expected, got);
    }

    #[test]
    fn soft_bits_are_signed_and_scaled_like_the_slicer_tier() {
        let params = gmsk(0.5, 3);
        let rx = gmsk_rx(0.5, 3);
        let response = SymbolResponse::of(&params, &rx);
        let observed = model_stream(&response, params.mapping(), &draws(300, 2, 0x5b17));
        let mut detector = MlseDetector::new(&params, &rx);
        let (mut got, mut bits) = (Vec::new(), Vec::new());
        detector.process(&observed, &mut got, &mut bits);
        detector.flush(&mut got, &mut bits);
        assert_eq!(bits.len(), got.len());
        let bits = &bits[..bits.len() - response.lead()];
        for (i, (&sym, bit)) in got.iter().zip(bits).enumerate().skip(32) {
            assert_eq!(
                bit.bit(),
                sym == 1,
                "bit {i} disagrees with its own hard decision"
            );
            assert!(
                bit.0.abs() <= 1.0,
                "bit {i} soft value {} past scale",
                bit.0
            );
        }
        let confident = bits[32..].iter().filter(|b| b.0.abs() > 0.4).count();
        assert!(
            confident > (bits.len() - 32) / 2,
            "only {confident} of {} soft bits carried real confidence",
            bits.len() - 32
        );
    }

    #[test]
    fn a_mis_scaled_input_still_decodes() {
        let params = gmsk(0.3, 4);
        let rx = gmsk_rx(0.3, 4);
        let response = SymbolResponse::of(&params, &rx);
        let sent = draws(800, 2, 0x2ea1);
        let clean = model_stream(&response, params.mapping(), &sent);
        for scale in [0.75f32, 1.0, 1.4] {
            let observed: Vec<f32> = clean.iter().map(|&y| y * scale).collect();
            let mut detector = MlseDetector::new(&params, &rx);
            let got = detect(&mut detector, &observed);
            let errors = (300..sent.len() - response.lead())
                .filter(|&i| got[i] != sent[i])
                .count();
            assert_eq!(errors, 0, "scale {scale}: {errors} errors after settling");
        }
    }

    #[test]
    #[should_panic(expected = "past the")]
    fn an_unreachable_trellis_is_a_construction_error() {
        let params = CpmParams::from_h(
            Mapping::natural(8),
            0.25,
            pulse::lrec(SPS, 6, Norm::Area),
            SPS,
        );
        let _ = MlseDetector::new(&params, &pulse::lrec(SPS, 6, Norm::Area));
    }

    #[test]
    fn steady_state_detection_allocates_nothing() {
        let params = gmsk(0.3, 4);
        let rx = gmsk_rx(0.3, 4);
        let response = SymbolResponse::of(&params, &rx);
        let observed = model_stream(&response, params.mapping(), &draws(1_200, 2, 0x0dd5));
        let mut detector = MlseDetector::new(&params, &rx);
        let mut got = Vec::with_capacity(observed.len() * 2);
        let mut bits = Vec::with_capacity(observed.len() * 2);
        detector.process(&observed, &mut got, &mut bits);
        got.clear();
        bits.clear();
        detector.process(&observed, &mut got, &mut bits);
        got.clear();
        bits.clear();
        sdrmm_modem_test_support::ber::perf::assert_no_alloc("MlseDetector::process", || {
            detector.process(&observed, &mut got, &mut bits);
        });
        assert!(!got.is_empty(), "the measured call decided nothing");
    }
}
