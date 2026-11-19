//! The Q65 codec: a QRA(15, 65) repeat-accumulate code over GF(64) carrying 13 message
//! symbols and a punctured CRC-12, decoded by non-binary message passing.

use sdrmm_dsp::math::walsh::walsh_hadamard;

use super::tables::{
    ACC_INPUT, ACC_WEIGHT_LOG, FACTOR_DEGREE, FACTOR_MESSAGES, LORENTZ, LORENTZ_START, NONE,
    VARIABLE_DEGREE, VARIABLE_MESSAGES,
};

pub(super) const ALPHABET: usize = 64;
pub(super) const MESSAGE_SYMBOLS: usize = 13;
pub(super) const CODEWORD_SYMBOLS: usize = 63;
const INFO: usize = 15;
const VARIABLES: usize = 65;
const FACTORS: usize = 116;
const CHECKS: usize = VARIABLES - INFO;
const MESSAGES: usize = 216;
const MAX_VARIABLE_DEGREE: usize = 5;
const MAX_FACTOR_DEGREE: usize = 3;
const FIELD_ORDER: usize = ALPHABET - 1;
const PRIMITIVE: usize = 0x43;
const CRC12: u32 = 0xF01;
const EB_NO_METRIC_DB: f32 = 2.8;
const QRA64_SYMBOL_S: f32 = 0.576;
const LLH_THRESHOLD: f32 = -260.0;
const UNDERFLOW_BIAS: f32 = 1e-7;
const LOG_LIMIT: f32 = 85.0;
const CONVERGED_SLACK: f32 = 0.01;
const LORENTZ_TABLES: usize = 64;

type Distribution = [f32; ALPHABET];

/// Logarithms and powers of the field's primitive element.
struct Field {
    exp: [u8; FIELD_ORDER],
    log: [u8; ALPHABET],
}

impl Field {
    const fn new() -> Self {
        let mut exp = [0u8; FIELD_ORDER];
        let mut log = [0u8; ALPHABET];
        let mut value = 1usize;
        let mut power = 0;
        while power < FIELD_ORDER {
            exp[power] = value as u8;
            log[value] = power as u8;
            value <<= 1;
            if value & ALPHABET != 0 {
                value ^= PRIMITIVE;
            }
            power += 1;
        }
        Self { exp, log }
    }

    /// `x` times the primitive element raised to `power`.
    fn scale(&self, x: u8, power: usize) -> u8 {
        if x == 0 {
            0
        } else {
            self.exp[(usize::from(self.log[usize::from(x)]) + power) % FIELD_ORDER]
        }
    }
}

const FIELD: Field = Field::new();

/// The weight of each graph message, as a power of the primitive element: the accumulator's
/// edges carry the code's weights, every other edge none.
fn message_weight(message: usize) -> usize {
    message
        .checked_sub(VARIABLES)
        .and_then(|edge| ACC_WEIGHT_LOG.get(edge))
        .map_or(0, |&weight| usize::from(weight))
}

/// The CRC-12 of `symbols`, as the two symbols that follow them.
fn crc12(symbols: &[u8]) -> [u8; 2] {
    let mut register = 0u32;
    for &symbol in symbols {
        let mut bits = u32::from(symbol);
        for _ in 0..6 {
            register = if (bits ^ register) & 1 == 1 {
                (register >> 1) ^ CRC12
            } else {
                register >> 1
            };
            bits >>= 1;
        }
    }
    [(register & 0x3F) as u8, (register >> 6) as u8]
}

/// The 63 transmitted symbols of `message`: the message itself, then the 50 check symbols;
/// the two CRC symbols are punctured.
pub(super) fn encode(message: &[u8; MESSAGE_SYMBOLS]) -> [u8; CODEWORD_SYMBOLS] {
    let mut info = [0u8; INFO];
    info[..MESSAGE_SYMBOLS].copy_from_slice(message);
    let crc = crc12(message);
    info[MESSAGE_SYMBOLS..].copy_from_slice(&crc);
    let mut codeword = [0u8; CODEWORD_SYMBOLS];
    codeword[..MESSAGE_SYMBOLS].copy_from_slice(message);
    let mut check = 0u8;
    for (slot, (&input, &weight)) in codeword[MESSAGE_SYMBOLS..]
        .iter_mut()
        .zip(ACC_INPUT.iter().zip(&ACC_WEIGHT_LOG))
        .take(CHECKS)
    {
        check ^= FIELD.scale(info[usize::from(input)], usize::from(weight));
        *slot = check;
    }
    codeword
}

/// The Es/No the intrinsic metric is tuned for, from the Eb/No that halves the frame error rate.
fn metric_es_no() -> f32 {
    let rate = MESSAGE_SYMBOLS as f32 / CODEWORD_SYMBOLS as f32;
    6.0 * rate * 10f32.powf(EB_NO_METRIC_DB / 10.0)
}

/// Each symbol's value probabilities from the energies of its `64 * (2 + bins_per_tone)` bins,
/// the first tone at bin 64, for a Lorentzian Doppler spread of `spread_symbols` bins.
pub(super) fn intrinsics(
    energies: &[f32],
    submode: u8,
    spread_symbols: f32,
    probabilities: &mut [Distribution; CODEWORD_SYMBOLS],
) {
    let spread = spread_symbols / QRA64_SYMBOL_S;
    let table = ((spread.ln() / 1.09f32.ln() - 0.499) as isize)
        .clamp(0, LORENTZ_TABLES as isize - 1) as usize;
    let gains = &LORENTZ[usize::from(LORENTZ_START[table])..usize::from(LORENTZ_START[table + 1])];
    let es_no = metric_es_no() * 10f32.powf(0.8 * spread.ln() / 240f32.ln());
    let bins_per_tone = 1usize << submode;
    let bins_per_symbol = ALPHABET * (2 + bins_per_tone);
    let total = (bins_per_symbol * CODEWORD_SYMBOLS).min(energies.len());
    let mean = energies[..total].iter().sum::<f32>() / total.max(1) as f32;
    let noise = (mean / (1.0 + es_no / bins_per_symbol as f32)).max(f32::MIN_POSITIVE);
    let weights: Vec<f32> = gains
        .iter()
        .map(|&gain| {
            let scaled = gain * es_no;
            scaled / (1.0 + scaled) / noise
        })
        .collect();
    let taps = weights.len();
    for (symbol, distribution) in probabilities.iter_mut().enumerate() {
        let center = symbol * bins_per_symbol + ALPHABET;
        for (tone, value) in distribution.iter_mut().enumerate() {
            let first = center + tone * bins_per_tone + 1 - taps;
            *value = weighted_energy(energies, first, &weights);
        }
        softmax(distribution);
    }
}

fn weighted_energy(energies: &[f32], first: usize, weights: &[f32]) -> f32 {
    let last = 2 * (weights.len() - 1);
    let bin = |offset: usize| energies.get(first + offset).copied().unwrap_or(0.0);
    let (center, sides) = weights
        .split_last()
        .map_or((0.0, &[][..]), |(c, s)| (*c, s));
    sides
        .iter()
        .enumerate()
        .map(|(tap, weight)| weight * (bin(tap) + bin(last - tap)))
        .sum::<f32>()
        + center * bin(weights.len() - 1)
}

fn softmax(distribution: &mut Distribution) {
    let peak = distribution.iter().copied().fold(0.0f32, f32::max);
    let mut total = 0.0;
    for value in distribution.iter_mut() {
        *value = (*value - peak).clamp(-LOG_LIMIT, LOG_LIMIT).exp();
        total += *value;
    }
    for value in distribution.iter_mut() {
        *value /= total;
    }
}

/// The message passing state, kept between decodes so a decode does not allocate.
pub(super) struct Decoder {
    to_factor: Vec<Distribution>,
    to_variable: Vec<Distribution>,
    intrinsic: Vec<Distribution>,
}

impl Decoder {
    pub(super) fn new() -> Self {
        Self {
            to_factor: vec![[0.0; ALPHABET]; MESSAGES],
            to_variable: vec![[0.0; ALPHABET]; MESSAGES],
            intrinsic: vec![[0.0; ALPHABET]; VARIABLES],
        }
    }

    /// The 13 message symbols, when message passing converges within `iterations`, the CRC
    /// holds and the codeword is likely enough given `probabilities`.
    pub(super) fn decode(
        &mut self,
        probabilities: &[Distribution; CODEWORD_SYMBOLS],
        iterations: usize,
    ) -> Option<([u8; MESSAGE_SYMBOLS], [u8; CODEWORD_SYMBOLS])> {
        self.depuncture(probabilities);
        if !self.pass_messages(iterations) {
            return None;
        }
        let mut info = [0u8; INFO];
        for (symbol, value) in info.iter_mut().enumerate() {
            let posterior = &mut self.to_factor[symbol];
            for (p, q) in posterior.iter_mut().zip(&self.intrinsic[symbol]) {
                *p *= q;
            }
            *value = argmax(posterior);
        }
        let message: [u8; MESSAGE_SYMBOLS] = std::array::from_fn(|i| info[i]);
        if crc12(&message) != [info[MESSAGE_SYMBOLS], info[MESSAGE_SYMBOLS + 1]]
            || message.iter().all(|&symbol| symbol == 0)
        {
            return None;
        }
        let codeword = encode(&message);
        (log_likelihood(&codeword, probabilities) >= LLH_THRESHOLD).then_some((message, codeword))
    }

    fn depuncture(&mut self, probabilities: &[Distribution; CODEWORD_SYMBOLS]) {
        let uniform = [1.0 / ALPHABET as f32; ALPHABET];
        self.intrinsic[..MESSAGE_SYMBOLS].copy_from_slice(&probabilities[..MESSAGE_SYMBOLS]);
        self.intrinsic[MESSAGE_SYMBOLS] = uniform;
        self.intrinsic[MESSAGE_SYMBOLS + 1] = uniform;
        self.intrinsic[INFO..].copy_from_slice(&probabilities[MESSAGE_SYMBOLS..]);
    }

    fn pass_messages(&mut self, iterations: usize) -> bool {
        self.to_variable[..VARIABLES].copy_from_slice(&self.intrinsic);
        for variable in 0..VARIABLES {
            for &message in variable_messages(variable).iter().skip(1) {
                self.to_factor[usize::from(message)] = self.intrinsic[variable];
            }
        }
        for _ in 0..iterations {
            for factor in VARIABLES..FACTORS {
                self.factor_step(factor);
            }
            for variable in 0..VARIABLES {
                self.variable_step(variable);
            }
            let total: f32 = self.to_factor[..VARIABLES]
                .iter()
                .map(|distribution| distribution.iter().copied().fold(0.0, f32::max))
                .sum();
            if total > VARIABLES as f32 - CONVERGED_SLACK {
                return true;
            }
        }
        false
    }

    fn factor_step(&mut self, factor: usize) {
        let messages = factor_messages(factor);
        for &message in messages {
            walsh_hadamard(&mut self.to_factor[usize::from(message)]);
        }
        for (k, &out) in messages.iter().enumerate() {
            let mut product = [1.0 / ALPHABET as f32; ALPHABET];
            for (kk, &other) in messages.iter().enumerate() {
                if kk != k {
                    multiply(&mut product, &self.to_factor[usize::from(other)]);
                }
            }
            product[0] += UNDERFLOW_BIAS;
            walsh_hadamard(&mut product);
            let out = usize::from(out);
            let weight = message_weight(out);
            let target = &mut self.to_variable[out];
            for (value, &p) in product.iter().enumerate() {
                target[usize::from(FIELD.scale(value as u8, FIELD_ORDER - weight))] = p;
            }
        }
    }

    fn variable_step(&mut self, variable: usize) {
        let messages = variable_messages(variable);
        for (k, &out) in messages.iter().enumerate() {
            let mut product = [1.0 / ALPHABET as f32; ALPHABET];
            for (kk, &other) in messages.iter().enumerate() {
                if kk != k {
                    multiply(&mut product, &self.to_variable[usize::from(other)]);
                }
            }
            normalize(&mut product);
            let out = usize::from(out);
            let weight = message_weight(out);
            let target = &mut self.to_factor[out];
            for (value, slot) in target.iter_mut().enumerate() {
                *slot = product[usize::from(FIELD.scale(value as u8, FIELD_ORDER - weight))];
            }
        }
    }
}

fn variable_messages(variable: usize) -> &'static [u8] {
    let row = &VARIABLE_MESSAGES[variable * MAX_VARIABLE_DEGREE..][..MAX_VARIABLE_DEGREE];
    &row[..usize::from(VARIABLE_DEGREE[variable])]
}

fn factor_messages(factor: usize) -> &'static [u8] {
    let row = &FACTOR_MESSAGES[factor * MAX_FACTOR_DEGREE..][..MAX_FACTOR_DEGREE];
    let degree = usize::from(FACTOR_DEGREE[factor]);
    debug_assert!(row[..degree].iter().all(|&m| m != NONE));
    &row[..degree]
}

fn multiply(product: &mut Distribution, factor: &Distribution) {
    for (p, f) in product.iter_mut().zip(factor) {
        *p *= f;
    }
}

fn normalize(distribution: &mut Distribution) {
    let total: f32 = distribution.iter().sum();
    if total > 0.0 {
        for value in distribution.iter_mut() {
            *value /= total;
        }
    } else {
        *distribution = [1.0 / ALPHABET as f32; ALPHABET];
    }
}

fn argmax(distribution: &Distribution) -> u8 {
    let mut best = 0;
    for (index, &value) in distribution.iter().enumerate() {
        if value > distribution[best] {
            best = index;
        }
    }
    best as u8
}

fn log_likelihood(
    codeword: &[u8; CODEWORD_SYMBOLS],
    probabilities: &[Distribution; CODEWORD_SYMBOLS],
) -> f32 {
    codeword
        .iter()
        .zip(probabilities)
        .map(|(&value, distribution)| distribution[usize::from(value)].max(1e-36).ln())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_field_matches_the_wsjt_x_tables() {
        assert_eq!(
            &FIELD.exp[..12],
            &[1, 2, 4, 8, 16, 32, 3, 6, 12, 24, 48, 35]
        );
        assert_eq!(FIELD.exp[62], 33);
        assert_eq!(FIELD.scale(1, FIELD_ORDER - 1), 33);
        assert_eq!(FIELD.scale(5, FIELD_ORDER - 1), 35);
        assert_eq!(FIELD.scale(1, FIELD_ORDER - 2), 49);
        assert_eq!(FIELD.scale(2, FIELD_ORDER - 2), 33);
        assert_eq!(
            [66, 69, 70, 115, 65, 116].map(message_weight),
            [14, 13, 37, 17, 0, 0]
        );
    }

    #[test]
    fn the_crc_is_zero_for_a_zero_message_and_sees_one_bit() {
        assert_eq!(crc12(&[0; MESSAGE_SYMBOLS]), [0, 0]);
        let mut one = [0u8; MESSAGE_SYMBOLS];
        one[12] = 2;
        assert_ne!(crc12(&one), [0, 0]);
    }

    fn noisy(codeword: &[u8; CODEWORD_SYMBOLS], wrong: usize) -> [Distribution; CODEWORD_SYMBOLS] {
        let mut probabilities = [[0.5 / 63.0; ALPHABET]; CODEWORD_SYMBOLS];
        for (symbol, (distribution, &value)) in probabilities.iter_mut().zip(codeword).enumerate() {
            let heard = if symbol % 7 == 3 && symbol / 7 < wrong {
                (value + 17) % 64
            } else {
                value
            };
            distribution[usize::from(heard)] = 0.5;
        }
        probabilities
    }

    #[test]
    fn message_passing_recovers_a_message_through_symbol_errors() {
        let message = [7, 0, 63, 12, 33, 1, 2, 3, 40, 41, 42, 9, 4];
        let codeword = encode(&message);
        let mut decoder = Decoder::new();
        let (decoded, rebuilt) = decoder.decode(&noisy(&codeword, 6), 60).expect("decodes");
        assert_eq!(decoded, message);
        assert_eq!(rebuilt, codeword);
    }

    #[test]
    fn uniform_probabilities_decode_to_nothing() {
        let mut decoder = Decoder::new();
        let flat = [[1.0 / 64.0; ALPHABET]; CODEWORD_SYMBOLS];
        assert!(decoder.decode(&flat, 60).is_none());
    }
}
