use sdrmm_dsp::coding::fec::fano::{Fano, FanoCode};

use super::message::PAYLOAD_BITS;

pub(crate) const CODED_BITS: usize = 162;
const TAIL_BITS: usize = 31;
const POLYNOMIALS: [u32; 2] = [0xf2d0_5351, 0xe461_3c47];
const CODE: FanoCode<'static> = FanoCode {
    polynomials: &POLYNOMIALS,
    data: PAYLOAD_BITS,
    tail: TAIL_BITS,
};

pub(crate) const SYNC: &[u8; CODED_BITS] = b"110000001000111000100101111000000010010100000010110011010001101000011010101010010010110001101010001000001001001110110011010001110000010100110000000110101100011000";

pub(crate) fn sync_bit(symbol: usize) -> usize {
    usize::from(SYNC[symbol] - b'0')
}

pub(crate) fn encode(payload: u64) -> [u8; CODED_BITS] {
    let mut coded = [0u8; CODED_BITS];
    let bits = (0..PAYLOAD_BITS).map(|index| payload >> (PAYLOAD_BITS - 1 - index) & 1 == 1);
    CODE.encode(bits, &mut coded);
    coded
}

pub(crate) fn interleaved_positions() -> [usize; CODED_BITS] {
    let mut positions = [0usize; CODED_BITS];
    let mut next = 0;
    for index in 0..=255u8 {
        let reversed = usize::from(index.reverse_bits());
        if reversed < CODED_BITS {
            positions[next] = reversed;
            next += 1;
        }
    }
    positions
}

pub(crate) fn tones(payload: u64) -> [u8; CODED_BITS] {
    let coded = encode(payload);
    let mut tones = [0u8; CODED_BITS];
    for (bit, &symbol) in coded.iter().zip(&interleaved_positions()) {
        tones[symbol] = SYNC[symbol] - b'0' + 2 * bit;
    }
    tones
}

pub(crate) fn bit_metrics(llr: f32) -> [f32; 2] {
    CODE.metric(llr)
}

pub(crate) fn decoder() -> Fano<'static> {
    Fano::new(CODE)
}

pub(crate) fn decode(
    fano: &mut Fano,
    metrics: &[[f32; 2]; CODED_BITS],
    delta: f32,
    max_cycles: usize,
) -> Option<u64> {
    fano.decode(metrics, delta, max_cycles).then(|| {
        fano.data()
            .fold(0u64, |acc, bit| (acc << 1) | u64::from(bit))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics_for(coded: &[u8; CODED_BITS], llr: impl Fn(usize) -> f32) -> [[f32; 2]; CODED_BITS] {
        std::array::from_fn(|index| {
            let sign = if coded[index] == 1 { 1.0 } else { -1.0 };
            bit_metrics(sign * llr(index))
        })
    }

    #[test]
    fn a_clean_codeword_decodes() {
        let payload = 0x2_3456_789a_bcdeu64 & ((1 << PAYLOAD_BITS) - 1);
        let coded = encode(payload);
        let decoded = decode(&mut decoder(), &metrics_for(&coded, |_| 4.0), 1.0, 10_000);
        assert_eq!(decoded, Some(payload));
    }

    #[test]
    fn scattered_errors_are_corrected() {
        let payload = 0x1_f00d_cafe_beefu64 & ((1 << PAYLOAD_BITS) - 1);
        let coded = encode(payload);
        let flipped = [3, 17, 40, 41, 77, 100, 130, 155];
        let metrics = metrics_for(
            &coded,
            |index| {
                if flipped.contains(&index) { -1.5 } else { 2.0 }
            },
        );
        assert_eq!(
            decode(&mut decoder(), &metrics, 1.0, 100_000),
            Some(payload)
        );
    }

    #[test]
    fn soft_decisions_decode_near_the_sequential_decoding_limit() {
        let mut seed = 1u64;
        let mut gaussian = || {
            let mut total = 0.0;
            for _ in 0..12 {
                seed = seed
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                total += (seed >> 40) as f32 / (1u64 << 24) as f32;
            }
            total - 6.0
        };
        let ebn0 = 10f32.powf(0.3);
        let sigma = (1.0 / (2.0 * ebn0 * PAYLOAD_BITS as f32 / CODED_BITS as f32)).sqrt();
        let mut fano = decoder();
        let decoded = (0..40u64)
            .filter(|trial| {
                let payload = trial.wrapping_mul(0x9e37_79b9_7f4a_7c15) & ((1 << PAYLOAD_BITS) - 1);
                let coded = encode(payload);
                let metrics: [[f32; 2]; CODED_BITS] = std::array::from_fn(|index| {
                    let sign = if coded[index] == 1 { 1.0 } else { -1.0 };
                    bit_metrics(2.0 * (sign + sigma * gaussian()) / (sigma * sigma))
                });
                decode(&mut fano, &metrics, 1.0, 1_000_000) == Some(payload)
            })
            .count();
        assert!(decoded >= 38, "{decoded}/40 at Eb/N0 3 dB");
    }

    #[test]
    fn every_symbol_is_interleaved_exactly_once() {
        let mut seen = [false; CODED_BITS];
        for position in interleaved_positions() {
            assert!(!seen[position]);
            seen[position] = true;
        }
    }
}
