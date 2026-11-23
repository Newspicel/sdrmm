use std::{
    f32::consts::{FRAC_1_SQRT_2, FRAC_PI_2, PI},
    f64::consts::TAU,
};

use num_complex::Complex;
use sdrmm_dsp::{Biquad, FracDelay, MovingAverage, design_rrc_unit_energy, fast_arg};

use super::{
    acquisition::{CoarseAcquisition, Lock},
    frame::{FrameDecoder, FrameHeader, HEADER_BITS, HIGH_RATE_BPS, UW},
    framer::DecodedFrame,
    taps::Fir,
};

pub(super) const BIT_RATE: u32 = HIGH_RATE_BPS;
pub(super) const CHANNEL_RATE_HR: f64 = 48_000.0;
pub(super) const HR_SKIP_BITS: usize = 16 + 178;
pub(super) const HR_CODED_BITS: usize = 64 * 78;
const STROBE_POINT: f64 = 0.85;
const LOCK_MSE: f32 = 0.5;
const LOCK_QUAD: f32 = 0.4;
const ACQ_FFT: usize = 16_384;
const UNLOCKED_MSE: f32 = 1.0;
const CLEAN_MSE: f32 = 0.1;
const RRC_TAPS: usize = 55;
const AGC_CLIP: f32 = 2.84;
const BIAS_WINDOW: usize = 800;
const UW_TOLERANCE: u32 = 2;
const GARDNER_GAIN: f64 = 0.005;
const HIGH_RATE_RESONATOR_HZ: f64 = 5.0;
const C_CHANNEL_RESONATOR_HZ: f64 = 20.0;
const CARRIER_LOOP: ([f32; 3], [f32; 2]) = (
    [0.001_027_561, 0.002_055_122, 0.001_027_561],
    [-1.920_738_7, 0.925_092_46],
);

const fn biquad((b, a): ([f32; 3], [f32; 2])) -> Biquad {
    Biquad::from_coefficients(b, a)
}

/// A band-pass resonator on the bit-rate tone whose phase there is zero, so one strobe point
/// fits every rate.
fn timing_resonator(bit_rate: f64, fs: f64, bandwidth_hz: f64) -> Biquad {
    let radius = (-std::f64::consts::PI * bandwidth_hz / fs).exp();
    let gain = (1.0 - radius * radius) / 2.0;
    let a1 = -2.0 * radius * (TAU * bit_rate / fs).cos();
    biquad((
        [gain as f32, 0.0, -gain as f32],
        [a1 as f32, (radius * radius) as f32],
    ))
}

struct SymbolTiming {
    power_delay: FracDelay,
    quarter_first: FracDelay,
    quarter_second: FracDelay,
    eighth: FracDelay,
    resonator: Biquad,
    phase: f64,
    strobe_phase: f64,
    pending: f64,
    freq_hz: f64,
    bit_rate: f64,
}

impl SymbolTiming {
    fn new(samples_per_symbol: f64, bit_rate: f64, resonator: Biquad) -> Self {
        Self {
            power_delay: FracDelay::new(1.0),
            quarter_first: FracDelay::new(samples_per_symbol / 4.0),
            quarter_second: FracDelay::new(samples_per_symbol / 4.0),
            eighth: FracDelay::new(samples_per_symbol / 8.0),
            resonator,
            phase: 0.0,
            strobe_phase: 1.0 - STROBE_POINT,
            pending: 0.0,
            freq_hz: bit_rate,
            bit_rate,
        }
    }

    fn strobe_fraction(&mut self, sample: Complex<f32>, fs: f64) -> Option<f32> {
        let power = sample.norm_sqr();
        let difference = self.power_delay.push(power) - power;
        let first = self.quarter_first.push(difference);
        let second = self.quarter_second.push(first);
        let eta = self.resonator.run((second - difference) * first);
        let detector = Complex::new(eta, -self.eighth.push(eta));
        let rotation = Complex::from_polar(1.0, (TAU * self.phase) as f32);
        let error = f64::from(fast_arg(rotation * detector));
        self.freq_hz =
            (self.freq_hz - error * 1e-8).clamp(self.bit_rate - 0.1, self.bit_rate + 0.1);
        let advance = self.freq_hz / fs - error * 0.01 / 360.0;
        self.phase = (self.phase + advance).rem_euclid(1.0);
        self.strobe_phase += advance + std::mem::take(&mut self.pending);
        if self.strobe_phase < 1.0 {
            return None;
        }
        self.strobe_phase = self.strobe_phase.rem_euclid(1.0);
        let step = self.freq_hz / fs;
        Some((self.strobe_phase / step).clamp(0.0, 1.0) as f32)
    }

    /// Shifts the strobe against the envelope tone through the next step, so a nudge never
    /// fires a strobe twice; a positive error means the strobe is late.
    fn nudge(&mut self, error: f32) {
        self.pending += f64::from(error.clamp(-1.0, 1.0)) * GARDNER_GAIN;
    }
}

pub(super) struct OqpskDemod {
    fs: f64,
    rrc: Fir,
    nco_freq_hz: f64,
    nco_phase: f64,
    phase_trim: f32,
    agc: f32,
    timing: SymbolTiming,
    last_sample: Complex<f32>,
    pair_toggle: bool,
    held: Complex<f32>,
    recent: [Complex<f32>; 2],
    carrier_loop: Biquad,
    bias: MovingAverage,
    mse: f32,
    quad: Complex<f32>,
    acquisition: CoarseAcquisition,
    points: Vec<Complex<f32>>,
}

impl OqpskDemod {
    pub(super) fn new(channel_rate: f64) -> Self {
        Self::with_rate(
            channel_rate,
            f64::from(BIT_RATE),
            HIGH_RATE_RESONATOR_HZ,
            1.0,
        )
    }

    pub(super) fn new_c_channel(channel_rate: f64) -> Self {
        Self::with_rate(channel_rate, 8_400.0, C_CHANNEL_RESONATOR_HZ, 0.6)
    }

    fn with_rate(channel_rate: f64, bit_rate: f64, resonator_hz: f64, rrc_beta: f64) -> Self {
        let samples_per_symbol = channel_rate / (bit_rate / 2.0);
        Self {
            fs: channel_rate,
            rrc: Fir::new(
                design_rrc_unit_energy(samples_per_symbol, rrc_beta, RRC_TAPS),
                1,
            ),
            nco_freq_hz: 0.0,
            nco_phase: 0.0,
            phase_trim: 0.0,
            agc: 1.0,
            timing: SymbolTiming::new(
                samples_per_symbol,
                bit_rate,
                timing_resonator(bit_rate, channel_rate, resonator_hz),
            ),
            last_sample: Complex::new(0.0, 0.0),
            pair_toggle: false,
            held: Complex::new(0.0, 0.0),
            recent: [Complex::new(0.0, 0.0); 2],
            carrier_loop: biquad(CARRIER_LOOP),
            bias: MovingAverage::new(BIAS_WINDOW),
            mse: UNLOCKED_MSE,
            quad: Complex::new(0.0, 0.0),
            acquisition: CoarseAcquisition::new(ACQ_FFT, channel_rate, bit_rate / 2.0, CLEAN_MSE),
            points: Vec::new(),
        }
    }

    /// The last block's symbol pairs after the carrier loop: QPSK at odd multiples of 45 degrees.
    pub(super) fn points(&self) -> &[Complex<f32>] {
        &self.points
    }

    pub(super) fn symbol_rate(&self) -> f64 {
        self.timing.bit_rate / 2.0
    }

    pub(super) fn frequency_error_hz(&self) -> f64 {
        self.nco_freq_hz
    }

    pub(super) fn locked(&self) -> bool {
        self.mse < LOCK_MSE && self.quad.norm() > LOCK_QUAD
    }

    pub(super) fn process(&mut self, input: &[Complex<f32>], out: &mut Vec<(f32, u8)>) {
        self.points.clear();
        for &raw in input {
            self.step(raw, out);
        }
    }

    fn step(&mut self, raw: Complex<f32>, out: &mut Vec<(f32, u8)>) {
        let rotation = Complex::from_polar(1.0, -self.nco_phase as f32 + self.phase_trim);
        self.nco_phase += TAU * self.nco_freq_hz / self.fs;
        if self.nco_phase > TAU {
            self.nco_phase -= TAU;
        }
        let filtered = self.rrc.filter(raw * rotation);
        if self.acquisition.push(raw) {
            let lock = Lock {
                locked: self.locked(),
                error: self.mse,
            };
            if let Some(carrier_hz) = self.acquisition.decide(self.nco_freq_hz, lock) {
                self.retune(carrier_hz);
            }
        }
        self.agc += 0.001 * (filtered.norm() - self.agc);
        let mut sample = filtered / self.agc.max(1e-9);
        let magnitude = sample.norm();
        if magnitude > AGC_CLIP {
            sample *= AGC_CLIP / magnitude;
        }
        if let Some(fraction) = self.timing.strobe_fraction(sample, self.fs) {
            let point = sample * (1.0 - fraction) + self.last_sample * fraction;
            self.strobe(point, out);
        }
        self.last_sample = sample;
    }

    fn retune(&mut self, carrier_hz: f64) {
        self.nco_freq_hz = carrier_hz;
        self.carrier_loop.reset();
        self.bias.reset();
        self.phase_trim = 0.0;
        self.mse = UNLOCKED_MSE;
        self.quad = Complex::new(0.0, 0.0);
    }

    fn strobe(&mut self, point: Complex<f32>, out: &mut Vec<(f32, u8)>) {
        self.pair_toggle = !self.pair_toggle;
        self.track_timing(point);
        if self.pair_toggle {
            self.held = point;
            return;
        }
        let mut pair = Complex::new(point.re, self.held.im);
        let cross =
            (self.held.re.tanh() * self.held.im - point.im.tanh() * point.re).clamp(-PI, PI);
        let error = self.carrier_loop.run(cross).clamp(-FRAC_PI_2, FRAC_PI_2);
        self.phase_trim += error.to_radians();
        self.nco_freq_hz -= 0.01 * f64::from(error);
        pair *= Complex::from_polar(1.0, self.bias.push(error));
        let ideal = Complex::new(
            FRAC_1_SQRT_2.copysign(pair.re),
            FRAC_1_SQRT_2.copysign(pair.im),
        );
        self.points.push(pair);
        self.mse += 0.0025 * ((pair - ideal).norm_sqr() - self.mse);
        let unit = pair / pair.norm().max(1e-9);
        self.quad += 0.0025 * ((unit * unit) * (unit * unit) - self.quad);
        if self.locked() && self.quad.re > LOCK_QUAD {
            self.slip_half_bit();
        }
        out.push((pair.im, u8::from(pair.im > 0.0)));
        out.push((pair.re, u8::from(pair.re > 0.0)));
    }

    /// Pairs on the axes instead of the diagonals mean a lock half a bit off with the carrier 45
    /// degrees out, a state structured data holds both loops in. A half-bit jump leaves it.
    fn slip_half_bit(&mut self) {
        self.timing.pending += 0.5;
        self.quad = Complex::new(0.0, 0.0);
    }

    /// Gardner on the rail centred at this strobe, whose transition sat on the one before. The
    /// envelope tone alone flips half a bit on unscrambled data; in lock the pairing names the
    /// rails, so this pulls the strobe onto the symbol centres whatever the data.
    fn track_timing(&mut self, point: Complex<f32>) {
        let [before, middle] = self.recent;
        self.recent = [middle, point];
        if !self.locked() {
            return;
        }
        let rail = |z: Complex<f32>| if self.pair_toggle { z.im } else { z.re };
        let error = rail(middle) * (rail(point) - rail(before));
        self.timing.nudge(error);
    }
}

fn check_uw(window: u64) -> Option<[f32; 2]> {
    let mut even = 0u32;
    let mut odd = 0u32;
    for k in 0..32 {
        even |= (((window >> (63 - 2 * k)) & 1) as u32) << (31 - k);
        odd |= (((window >> (63 - (2 * k + 1))) & 1) as u32) << (31 - k);
    }
    let sign = |rail: u32| {
        if (rail ^ UW).count_ones() <= UW_TOLERANCE {
            Some(1.0)
        } else if (rail ^ !UW).count_ones() <= UW_TOLERANCE {
            Some(-1.0)
        } else {
            None
        }
    };
    Some([sign(even)?, sign(odd)?])
}

pub(super) struct HrFramer {
    decoder: FrameDecoder,
    shift: u64,
    buffer: Vec<f32>,
    inversion: Option<[f32; 2]>,
}

impl HrFramer {
    pub(super) fn new() -> Self {
        Self {
            decoder: FrameDecoder::new(BIT_RATE),
            shift: 0,
            buffer: Vec::with_capacity(HR_SKIP_BITS + HR_CODED_BITS),
            inversion: None,
        }
    }

    pub(super) fn push(&mut self, soft: f32, hard: u8) -> Option<DecodedFrame> {
        let mut frame = None;
        if let Some(inversion) = self.inversion {
            let index = self.buffer.len();
            self.buffer.push(soft * inversion[index % 2]);
            if self.buffer.len() == HR_SKIP_BITS + HR_CODED_BITS {
                let header = FrameHeader::from_soft_bits(&self.buffer[..HEADER_BITS]);
                frame = Some(DecodedFrame::decode(
                    &mut self.decoder,
                    header,
                    &self.buffer[HR_SKIP_BITS..],
                ));
                self.buffer.clear();
                self.inversion = None;
            }
        }
        self.shift = (self.shift << 1) | u64::from(hard);
        if self.inversion.is_none() {
            self.inversion = check_uw(self.shift);
        }
        frame
    }
}

#[cfg(test)]
pub(super) use modulator::{hr_frame_bits, modulate_oqpsk, modulate_oqpsk_rate};

#[cfg(test)]
mod modulator {
    use sdrmm_dsp::filter::fir::rrc_pulse;

    use super::*;
    use crate::inmarsat_aero::frame::FrameEncoder;

    const PULSE_SPAN_SYMBOLS: f64 = 6.0;

    pub fn hr_frame_bits(encoder: &mut FrameEncoder, su_bytes: &[u8], counter: u8) -> Vec<u8> {
        let low = encoder.encode(su_bytes, counter);
        let mut bits = Vec::with_capacity(64 + HR_SKIP_BITS + HR_CODED_BITS);
        for k in 0..32 {
            let bit = ((UW >> (31 - k)) & 1) as u8;
            bits.push(bit);
            bits.push(bit);
        }
        bits.extend_from_slice(&low[32..48]);
        bits.extend(std::iter::repeat_n(0, HR_SKIP_BITS - 16));
        bits.extend_from_slice(&low[48..]);
        bits
    }

    pub fn modulate_oqpsk(
        bits: &[u8],
        sample_rate: f64,
        freq_offset_hz: f64,
        amplitude: f32,
    ) -> Vec<Complex<f32>> {
        modulate_oqpsk_rate(
            bits,
            f64::from(BIT_RATE),
            1.0,
            sample_rate,
            freq_offset_hz,
            amplitude,
        )
    }

    pub fn modulate_oqpsk_rate(
        bits: &[u8],
        bit_rate: f64,
        rrc_beta: f64,
        sample_rate: f64,
        freq_offset_hz: f64,
        amplitude: f32,
    ) -> Vec<Complex<f32>> {
        let samples_per_symbol = sample_rate / (bit_rate / 2.0);
        let half = samples_per_symbol / 2.0;
        let span = (PULSE_SPAN_SYMBOLS * samples_per_symbol).ceil() as i64;
        let energy = (-span..=span)
            .map(|n| rrc_pulse(n as f64 / samples_per_symbol, rrc_beta).powi(2))
            .sum::<f64>()
            .sqrt();
        let total = ((bits.len() + 4) as f64 * half) as usize;
        let mut rails = vec![[0.0f64; 2]; total];
        for (k, &bit) in bits.iter().enumerate() {
            let value = if bit == 1 { 1.0 } else { -1.0 } / energy;
            let center = (k + 1) as f64 * half + span as f64;
            let first = (center as i64 - span).max(0);
            let last = (center as i64 + span).min(total as i64 - 1);
            for n in first..=last {
                rails[n as usize][k % 2] +=
                    value * rrc_pulse((n as f64 - center) / samples_per_symbol, rrc_beta);
            }
        }
        rails
            .iter()
            .enumerate()
            .map(|(n, &[i, q])| {
                let phase = TAU * freq_offset_hz * n as f64 / sample_rate;
                Complex::new(i as f32, q as f32) * Complex::from_polar(amplitude, phase as f32)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Lcg(u64);

    impl Lcg {
        fn bit(&mut self) -> u8 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 33) & 1) as u8
        }
    }

    fn rail_ber(sent: &[u8], received: &[f32]) -> f64 {
        let mut best = 1.0f64;
        for lag in 0..256.min(received.len()) {
            let n = sent.len().min(received.len() - lag);
            if n < 2000 {
                break;
            }
            let mut errors = 0usize;
            let mut total = 0usize;
            for rail in 0..2 {
                let pairs: Vec<(f64, bool)> = (n / 2..n)
                    .filter(|k| k % 2 == rail)
                    .map(|k| (f64::from(received[lag + k]), sent[k] == 1))
                    .collect();
                let correlation: f64 = pairs
                    .iter()
                    .map(|&(soft, bit)| if bit { soft } else { -soft })
                    .sum();
                let sign = correlation.signum();
                errors += pairs
                    .iter()
                    .filter(|&&(soft, bit)| (soft * sign > 0.0) != bit)
                    .count();
                total += pairs.len();
            }
            best = best.min(errors as f64 / total as f64);
        }
        best
    }

    #[test]
    fn locks_and_demodulates_with_cfo() {
        for cfo in [0.0_f64, 120.0, -250.0] {
            let mut rng = Lcg(42);
            let bits: Vec<u8> = (0..40_000).map(|_| rng.bit()).collect();
            let iq = modulate_oqpsk(&bits, CHANNEL_RATE_HR, cfo, 0.5);
            let mut demod = OqpskDemod::new(CHANNEL_RATE_HR);
            let mut out = Vec::new();
            demod.process(&iq, &mut out);
            assert!(demod.locked(), "cfo={cfo}: mse {}", demod.mse);
            let soft: Vec<f32> = out.iter().map(|&(soft, _)| soft).collect();
            let ber = rail_ber(&bits, &soft);
            assert!(ber < 0.001, "cfo={cfo}: BER {ber}");
        }
    }

    fn hashed_bits(count: u32) -> Vec<u8> {
        (0..count)
            .map(|index| u8::from(index.wrapping_mul(2_654_435_761).rotate_left(7) & 4 != 0))
            .collect()
    }

    fn demod_for(bit_rate: f64) -> (OqpskDemod, f64) {
        if bit_rate == 8_400.0 {
            (OqpskDemod::new_c_channel(CHANNEL_RATE_HR), 0.6)
        } else {
            (OqpskDemod::new(CHANNEL_RATE_HR), 1.0)
        }
    }

    /// Frequency error once settled, and the bit error rate over the second half.
    fn settle(bits: &[u8], bit_rate: f64, cfo: f64) -> (bool, f64, f64) {
        let (mut demod, beta) = demod_for(bit_rate);
        let iq = modulate_oqpsk_rate(bits, bit_rate, beta, CHANNEL_RATE_HR, cfo, 0.5);
        let mut out = Vec::new();
        demod.process(&iq, &mut out);
        let soft: Vec<f32> = out.iter().map(|&(soft, _)| soft).collect();
        (
            demod.locked(),
            demod.frequency_error_hz() - cfo,
            rail_ber(bits, &soft),
        )
    }

    fn repeated(pattern: &[u8], seconds: f64, bit_rate: f64) -> Vec<u8> {
        let count = (seconds * bit_rate) as usize;
        pattern.iter().copied().cycle().take(count).collect()
    }

    #[test]
    fn pulls_in_a_carrier_at_the_edge_of_the_range() {
        for bit_rate in [8_400.0, f64::from(BIT_RATE)] {
            let mut rng = Lcg(7);
            let bits: Vec<u8> = (0..(4.0 * bit_rate) as usize).map(|_| rng.bit()).collect();
            for cfo in [-2_950.0, 2_500.0] {
                let (locked, error, ber) = settle(&bits, bit_rate, cfo);
                let case = format!("{bit_rate} bps, cfo {cfo}");
                assert!(locked, "{case}");
                assert!(error.abs() < 5.0, "{case}: off by {error} Hz");
                assert!(ber < 0.001, "{case}: BER {ber}");
            }
        }
    }

    /// Periodic data puts a comb of line pairs one pattern spacing apart.
    #[test]
    fn periodic_data_settles_on_the_true_carrier() {
        let mut rng = Lcg(4);
        let period_12: Vec<u8> = (0..12).map(|_| rng.bit()).collect();
        let period_20: Vec<u8> = (0..20).map(|_| rng.bit()).collect();
        for bit_rate in [8_400.0, f64::from(BIT_RATE)] {
            for (name, pattern) in [("12", &period_12), ("20", &period_20)] {
                let bits = repeated(pattern, 8.0, bit_rate);
                for cfo in [700.0, -1_400.0, -2_500.0, 2_950.0] {
                    let (locked, error, ber) = settle(&bits, bit_rate, cfo);
                    let case = format!("{bit_rate} bps, pattern {name}, cfo {cfo}");
                    assert!(locked, "{case}");
                    assert!(error.abs() < 5.0, "{case}: off by {error} Hz");
                    assert!(ber < 0.001, "{case}: BER {ber}");
                }
            }
        }
    }

    #[test]
    fn locks_on_unscrambled_bits_at_both_rates() {
        let bits = hashed_bits(40_000);
        for (bit_rate, beta) in [(8_400.0, 0.6), (f64::from(BIT_RATE), 1.0)] {
            for cfo in [0.0_f64, 60.0, -300.0, 700.0, 1_500.0] {
                let iq = modulate_oqpsk_rate(&bits, bit_rate, beta, CHANNEL_RATE_HR, cfo, 0.5);
                let mut demod = if bit_rate == 8_400.0 {
                    OqpskDemod::new_c_channel(CHANNEL_RATE_HR)
                } else {
                    OqpskDemod::new(CHANNEL_RATE_HR)
                };
                let mut out = Vec::new();
                demod.process(&iq, &mut out);
                let case = format!("{bit_rate} bps, cfo {cfo}");
                assert!(demod.locked(), "{case}: mse {}", demod.mse);
                let error = demod.frequency_error_hz() - cfo;
                assert!(error.abs() < 5.0, "{case}: off by {error} Hz");
                let soft: Vec<f32> = out.iter().map(|&(soft, _)| soft).collect();
                let ber = rail_ber(&bits, &soft);
                assert!(ber < 0.001, "{case}: BER {ber}");
            }
        }
    }
}
