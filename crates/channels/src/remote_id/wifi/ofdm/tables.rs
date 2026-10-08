use num_complex::Complex;

pub(crate) const FFT: usize = 64;
pub(crate) const CP: usize = 16;
pub(crate) const SYMBOL: usize = FFT + CP;
pub(crate) const SHORT: usize = 16;
pub(crate) const DATA_CARRIERS: usize = 48;
pub(crate) const PILOT_CARRIERS: [i32; 4] = [-21, -7, 7, 21];
pub(crate) const PILOT_VALUES: [f32; 4] = [1.0, 1.0, 1.0, -1.0];
pub(crate) const SERVICE_BITS: usize = 16;
pub(crate) const TAIL_BITS: usize = 6;
const LONG_TRAINING: [i8; 53] = [
    1, 1, -1, -1, 1, 1, -1, 1, -1, 1, 1, 1, 1, 1, 1, -1, -1, 1, 1, -1, 1, -1, 1, 1, 1, 1, 0, 1, -1,
    -1, 1, 1, -1, 1, -1, 1, -1, -1, -1, -1, -1, 1, 1, -1, -1, 1, -1, 1, -1, 1, 1, 1, 1,
];
const SHORT_TRAINING: [i8; 53] = [
    0, 0, 1, 0, 0, 0, -1, 0, 0, 0, 1, 0, 0, 0, -1, 0, 0, 0, -1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0,
    -1, 0, 0, 0, -1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Modulation {
    Bpsk,
    Qpsk,
    Qam16,
    Qam64,
}

impl Modulation {
    pub(crate) fn bits(self) -> usize {
        match self {
            Self::Bpsk => 1,
            Self::Qpsk => 2,
            Self::Qam16 => 4,
            Self::Qam64 => 6,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CodeRate {
    Half,
    TwoThirds,
    ThreeQuarters,
}

impl CodeRate {
    pub(crate) fn puncture(self) -> &'static [bool] {
        match self {
            Self::Half => &[true, true],
            Self::TwoThirds => &[true, true, true, false],
            Self::ThreeQuarters => &[true, true, true, false, false, true],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rate {
    pub code: u8,
    pub mbps: f32,
    pub modulation: Modulation,
    pub coding: CodeRate,
    pub data_bits: usize,
}

impl Rate {
    pub(crate) fn coded_bits(self) -> usize {
        DATA_CARRIERS * self.modulation.bits()
    }

    pub(crate) fn symbols(self, length: usize) -> usize {
        (SERVICE_BITS + 8 * length + TAIL_BITS).div_ceil(self.data_bits)
    }
}

pub(crate) const RATES: [Rate; 8] = [
    rate(0b1101, 6.0, Modulation::Bpsk, CodeRate::Half, 24),
    rate(0b1111, 9.0, Modulation::Bpsk, CodeRate::ThreeQuarters, 36),
    rate(0b0101, 12.0, Modulation::Qpsk, CodeRate::Half, 48),
    rate(0b0111, 18.0, Modulation::Qpsk, CodeRate::ThreeQuarters, 72),
    rate(0b1001, 24.0, Modulation::Qam16, CodeRate::Half, 96),
    rate(
        0b1011,
        36.0,
        Modulation::Qam16,
        CodeRate::ThreeQuarters,
        144,
    ),
    rate(0b0001, 48.0, Modulation::Qam64, CodeRate::TwoThirds, 192),
    rate(
        0b0011,
        54.0,
        Modulation::Qam64,
        CodeRate::ThreeQuarters,
        216,
    ),
];

const fn rate(
    code: u8,
    mbps: f32,
    modulation: Modulation,
    coding: CodeRate,
    data_bits: usize,
) -> Rate {
    Rate {
        code,
        mbps,
        modulation,
        coding,
        data_bits,
    }
}

pub(crate) fn rate_for(code: u8) -> Option<Rate> {
    RATES.iter().copied().find(|rate| rate.code == code)
}

pub(crate) fn bin(carrier: i32) -> usize {
    carrier.rem_euclid(FFT as i32) as usize
}

pub(crate) fn data_carriers() -> impl Iterator<Item = i32> {
    (-26..=26).filter(|carrier| *carrier != 0 && !PILOT_CARRIERS.contains(carrier))
}

pub(crate) fn long_training(carrier: i32) -> f32 {
    if (-26..=26).contains(&carrier) {
        f32::from(LONG_TRAINING[(carrier + 26) as usize])
    } else {
        0.0
    }
}

pub(crate) fn short_training(carrier: i32) -> Complex<f32> {
    let scale = (13.0f32 / 6.0).sqrt();
    if (-26..=26).contains(&carrier) {
        let value = f32::from(SHORT_TRAINING[(carrier + 26) as usize]);
        Complex::new(value, value) * scale
    } else {
        Complex::default()
    }
}

pub(crate) fn polarity() -> [f32; 127] {
    let mut register = 0x7Fu8;
    std::array::from_fn(|_| {
        let bit = (register >> 6 ^ register >> 3) & 1;
        register = (register << 1 | bit) & 0x7F;
        if bit == 1 { -1.0 } else { 1.0 }
    })
}

pub(crate) fn interleave_table(rate: Rate) -> Vec<usize> {
    let coded = rate.coded_bits();
    let spread = (rate.modulation.bits() / 2).max(1);
    (0..coded)
        .map(|k| {
            let i = (coded / 16) * (k % 16) + k / 16;
            spread * (i / spread) + (i + coded - (16 * i / coded)) % spread
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{RATES, data_carriers, interleave_table, polarity, rate_for};

    #[test]
    fn the_polarity_sequence_starts_as_the_standard_lists_it() {
        let p = polarity();
        let start: Vec<i8> = p[..16].iter().map(|&v| v as i8).collect();
        assert_eq!(
            start,
            [1, 1, 1, 1, -1, -1, -1, 1, -1, -1, -1, -1, 1, 1, -1, 1]
        );
        assert_eq!(p[126], -1.0);
    }

    #[test]
    fn interleaving_permutes_every_coded_bit() {
        for rate in RATES {
            let mut table = interleave_table(rate);
            table.sort_unstable();
            assert_eq!(table, (0..rate.coded_bits()).collect::<Vec<_>>());
        }
        assert_eq!(&interleave_table(RATES[0])[..4], &[0, 3, 6, 9]);
    }

    #[test]
    fn forty_eight_data_carriers_and_known_rates() {
        assert_eq!(data_carriers().count(), 48);
        assert_eq!(rate_for(0b1101).map(|rate| rate.mbps), Some(6.0));
        assert_eq!(rate_for(0b0000), None);
        assert_eq!(RATES[0].symbols(100), 35);
    }
}
