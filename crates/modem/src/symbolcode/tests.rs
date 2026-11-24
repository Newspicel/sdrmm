use sdrmm_modem_test_support::ber::rng::Rng;

use super::*;

fn random_bits(rng: &mut Rng, len: usize) -> Vec<bool> {
    (0..len).map(|_| rng.next_u64() & 1 == 1).collect()
}

fn payloads() -> Vec<Vec<bool>> {
    let mut rng = Rng::new(0x51b0);
    let mut set = vec![
        vec![],
        vec![false],
        vec![true],
        vec![false; 64],
        vec![true; 64],
    ];
    for len in [2, 3, 17, 256, 1001] {
        set.push(random_bits(&mut rng, len));
    }
    set
}

#[test]
fn nrz_is_the_identity_in_both_directions() {
    for payload in payloads() {
        assert_eq!(nrz_encode(&payload), payload);
        assert_eq!(nrz_decode(&nrz_encode(&payload)), payload);
    }
}

#[test]
fn nrzi_round_trips_in_both_conventions() {
    for convention in [
        NrziConvention::TransitionOnZero,
        NrziConvention::TransitionOnOne,
    ] {
        for payload in payloads() {
            let mut encoder = NrziEncoder::new(convention);
            let mut decoder = NrziDecoder::new(convention);
            let levels = encoder.encode_all(&payload);
            assert_eq!(decoder.decode_all(&levels), payload, "{convention:?}");

            encoder.reset();
            decoder.reset();
            assert_eq!(encoder.encode_all(&payload), levels, "{convention:?}");
            assert_eq!(decoder.decode_all(&levels), payload, "{convention:?}");
        }
    }
}

#[test]
fn nrzi_conventions_complement_each_other() {
    let mut rng = Rng::new(0x217);
    let payload = random_bits(&mut rng, 512);
    let inverted: Vec<bool> = payload.iter().map(|&b| !b).collect();
    let on_one = NrziEncoder::new(NrziConvention::TransitionOnOne).encode_all(&payload);
    let on_zero = NrziEncoder::new(NrziConvention::TransitionOnZero).encode_all(&inverted);
    assert_eq!(on_one, on_zero);
    assert_ne!(
        on_one,
        NrziEncoder::new(NrziConvention::TransitionOnZero).encode_all(&payload)
    );
}

#[test]
fn nrzi_transition_on_zero_agrees_with_the_dsp_decoder() {
    let mut rng = Rng::new(0xd5b);
    let levels = random_bits(&mut rng, 4096);
    let mut mine = NrziDecoder::new(NrziConvention::TransitionOnZero);
    let mut theirs = sdrmm_dsp::coding::bits::NrziDecoder::new();
    for &level in &levels {
        assert_eq!(mine.decode(level), theirs.decode(level));
    }

    let payload = random_bits(&mut rng, 2048);
    let mut encoder = NrziEncoder::new(NrziConvention::TransitionOnZero);
    let mut theirs = sdrmm_dsp::coding::bits::NrziDecoder::new();
    let decoded: Vec<bool> = encoder
        .encode_all(&payload)
        .iter()
        .map(|&l| theirs.decode(l))
        .collect();
    assert_eq!(decoded, payload);
}

#[test]
fn differential_round_trips() {
    for payload in payloads() {
        let mut encoder = DifferentialEncoder::new();
        let mut decoder = DifferentialDecoder::new();
        let encoded = encoder.encode_all(&payload);
        assert_eq!(decoder.decode_all(&encoded), payload);

        encoder.reset();
        decoder.reset();
        assert_eq!(encoder.encode_all(&payload), encoded);
        assert_eq!(decoder.decode_all(&encoded), payload);
    }
}

#[test]
fn differential_agrees_with_the_dsp_decoder() {
    let mut rng = Rng::new(0xd1ff);
    let stream = random_bits(&mut rng, 4096);
    let mut mine = DifferentialDecoder::new();
    let mut theirs = sdrmm_dsp::coding::bits::DifferentialDecoder::new();
    for &bit in &stream {
        assert_eq!(mine.decode(bit), theirs.decode(bit));
    }

    let payload = random_bits(&mut rng, 2048);
    let mut encoder = DifferentialEncoder::new();
    let mut theirs = sdrmm_dsp::coding::bits::DifferentialDecoder::new();
    let decoded: Vec<bool> = encoder
        .encode_all(&payload)
        .iter()
        .map(|&b| theirs.decode(b))
        .collect();
    assert_eq!(decoded, payload);
}

fn soft_levels(rng: &mut Rng, levels: &[bool]) -> Vec<f32> {
    levels
        .iter()
        .map(|&level| {
            let size = 0.1 + (rng.next_u64() % 100) as f32 / 50.0;
            if level { size } else { -size }
        })
        .collect()
}

#[test]
fn soft_differential_and_nrzi_decide_like_the_hard_ones_as_sure_as_the_weaker_level() {
    let mut rng = Rng::new(0x50f7);
    let levels = random_bits(&mut rng, 2048);
    let soft = soft_levels(&mut rng, &levels);
    let mut hard = DifferentialDecoder::new();
    let mut differential = SoftDifferentialDecoder::new();
    let mut nrzi = [
        NrziConvention::TransitionOnZero,
        NrziConvention::TransitionOnOne,
    ]
    .map(|convention| {
        (
            NrziDecoder::new(convention),
            SoftNrziDecoder::new(convention),
        )
    });
    for (index, (&level, &value)) in levels.iter().zip(&soft).enumerate() {
        let out = differential.decode(value);
        let decided = hard.decode(level);
        let nrzi_out = nrzi
            .each_mut()
            .map(|(hard, soft)| (hard.decode(level), soft.decode(value)));
        if index == 0 {
            assert_eq!(out, 0.0);
            continue;
        }
        assert_eq!(out > 0.0, decided, "bit {index}");
        assert_eq!(out.abs(), value.abs().min(soft[index - 1].abs()));
        for (hard, soft) in nrzi_out {
            assert_eq!(soft > 0.0, hard, "bit {index}");
            assert_eq!(soft.abs(), out.abs());
        }
    }
}

#[test]
fn mary_differential_round_trips() {
    let mut rng = Rng::new(0x3a17);
    for m in [2u32, 4, 8, 16, 64] {
        for len in [0usize, 1, 2, 500] {
            let symbols: Vec<u32> = (0..len)
                .map(|_| (rng.next_u64() % u64::from(m)) as u32)
                .collect();
            let mut encoder = DifferentialSymbolEncoder::new(m);
            let mut decoder = DifferentialSymbolDecoder::new(m);
            let encoded = encoder.encode_all(&symbols);
            assert_eq!(decoder.decode_all(&encoded), symbols, "M = {m}, len {len}");

            encoder.reset();
            decoder.reset();
            assert_eq!(encoder.encode_all(&symbols), encoded, "M = {m}");
            assert_eq!(decoder.decode_all(&encoded), symbols, "M = {m}");
        }
    }
}

#[test]
fn mary_differential_accumulates_increments() {
    let mut encoder = DifferentialSymbolEncoder::new(4);
    assert_eq!(encoder.encode_all(&[1, 1, 1, 3]), [1, 2, 3, 2]);
    let mut decoder = DifferentialSymbolDecoder::new(4);
    assert_eq!(decoder.decode_all(&[1, 2, 3, 2]), [1, 1, 1, 3]);
}

#[test]
fn binary_differential_is_the_m_equals_2_case() {
    let mut rng = Rng::new(0xb1);
    let payload = random_bits(&mut rng, 1024);
    let symbols: Vec<u32> = payload.iter().map(|&b| u32::from(b)).collect();

    let mut binary = DifferentialEncoder::new();
    let mut mary = DifferentialSymbolEncoder::new(2);
    let from_binary: Vec<u32> = binary
        .encode_all(&payload)
        .iter()
        .map(|&b| u32::from(b))
        .collect();
    assert_eq!(mary.encode_all(&symbols), from_binary);

    let mut binary = DifferentialDecoder::new();
    let mut mary = DifferentialSymbolDecoder::new(2);
    let from_binary: Vec<u32> = binary
        .decode_all(&payload)
        .iter()
        .map(|&b| u32::from(b))
        .collect();
    assert_eq!(mary.decode_all(&symbols), from_binary);
}

#[test]
fn manchester_pair_mapping_matches_the_conventions() {
    use ManchesterConvention::{GeThomas, Ieee8023};
    assert_eq!(manchester_decode_pair(GeThomas, true, false), Some(true));
    assert_eq!(manchester_decode_pair(GeThomas, false, true), Some(false));
    assert_eq!(manchester_decode_pair(Ieee8023, false, true), Some(true));
    assert_eq!(manchester_decode_pair(Ieee8023, true, false), Some(false));
    for convention in [Ieee8023, GeThomas] {
        assert_eq!(manchester_decode_pair(convention, true, true), None);
        assert_eq!(manchester_decode_pair(convention, false, false), None);
    }
}

#[test]
fn manchester_round_trips_in_both_conventions() {
    for convention in [
        ManchesterConvention::Ieee8023,
        ManchesterConvention::GeThomas,
    ] {
        for payload in payloads() {
            let levels = manchester_encode(convention, &payload);
            assert_eq!(levels.len(), payload.len() * 2);
            let decoded = manchester_decode(convention, &levels);
            assert_eq!(decoded.bits, payload, "{convention:?}");
            assert_eq!(decoded.violations, 0, "{convention:?}");
            assert_ne!(decoded.alignment, Alignment::MidBit, "{convention:?}");
            if payload.windows(2).any(|w| w[0] != w[1]) {
                assert_eq!(decoded.alignment, Alignment::BitEdge, "{convention:?}");
            }
        }
    }
}

#[test]
fn manchester_reports_a_half_bit_slip() {
    let payload: Vec<bool> = (0..64).map(|i| i % 2 == 0).collect();
    for convention in [
        ManchesterConvention::Ieee8023,
        ManchesterConvention::GeThomas,
    ] {
        let levels = manchester_encode(convention, &payload);
        let slipped = manchester_decode(convention, &levels[1..]);
        assert_eq!(slipped.alignment, Alignment::MidBit, "{convention:?}");
        assert!(slipped.violations > 0, "{convention:?}");

        let realigned = manchester_decode(convention, &levels[2..]);
        assert_eq!(realigned.bits, payload[1..]);
        assert_eq!(realigned.violations, 0);
    }
}

#[test]
fn manchester_flags_a_missing_transition_without_shortening_the_output() {
    let payload = vec![true, false, true, true, false];
    let mut levels = manchester_encode(ManchesterConvention::Ieee8023, &payload);
    levels[5] = levels[4];
    let decoded = manchester_decode(ManchesterConvention::Ieee8023, &levels);
    assert_eq!(decoded.violations, 1);
    assert_eq!(decoded.bits.len(), payload.len(), "positions stay aligned");
    assert_eq!(decoded.bits[..2], payload[..2]);
    assert_eq!(decoded.bits[3..], payload[3..]);
}

#[test]
fn ge_thomas_pairs_agree_with_the_dsp_decoder() {
    for first in [false, true] {
        for second in [false, true] {
            assert_eq!(
                manchester_decode_pair(ManchesterConvention::GeThomas, first, second),
                sdrmm_dsp::coding::bits::manchester_decode(first, second),
            );
        }
    }

    let mut rng = Rng::new(0xac);
    let payload = random_bits(&mut rng, 2048);
    let levels = manchester_encode(ManchesterConvention::GeThomas, &payload);
    let decoded: Vec<bool> = levels
        .as_chunks::<2>()
        .0
        .iter()
        .filter_map(|p| sdrmm_dsp::coding::bits::manchester_decode(p[0], p[1]))
        .collect();
    assert_eq!(decoded, payload);
}

#[test]
fn biphase_round_trips_in_both_variants() {
    for variant in [BiphaseVariant::Mark, BiphaseVariant::Space] {
        for payload in payloads() {
            let mut encoder = BiphaseEncoder::new(variant);
            let levels = encoder.encode_all(&payload);
            assert_eq!(levels.len(), payload.len() * 2);
            let decoded = biphase_decode(variant, &levels);
            assert_eq!(decoded.bits, payload, "{variant:?}");
            assert_eq!(decoded.violations, 0, "{variant:?}");
            assert_ne!(decoded.alignment, Alignment::MidBit, "{variant:?}");

            encoder.reset();
            assert_eq!(encoder.encode_all(&payload), levels, "{variant:?}");
        }
    }
}

#[test]
fn biphase_transitions_at_every_boundary_and_encodes_data_mid_bit() {
    let mut rng = Rng::new(0xf0);
    let payload = random_bits(&mut rng, 256);
    for variant in [BiphaseVariant::Mark, BiphaseVariant::Space] {
        let mut encoder = BiphaseEncoder::new(variant);
        let levels = encoder.encode_all(&payload);
        for (i, pair) in levels.as_chunks::<2>().0.iter().enumerate() {
            if i > 0 {
                assert_ne!(levels[2 * i - 1], pair[0], "boundary {i} must transition");
            }
            let expect_mid = match variant {
                BiphaseVariant::Mark => payload[i],
                BiphaseVariant::Space => !payload[i],
            };
            assert_eq!(pair[0] != pair[1], expect_mid, "{variant:?} bit {i}");
        }
    }
}

#[test]
fn biphase_is_polarity_free() {
    let mut rng = Rng::new(0x9019);
    let payload = random_bits(&mut rng, 256);
    for variant in [BiphaseVariant::Mark, BiphaseVariant::Space] {
        let levels = BiphaseEncoder::new(variant).encode_all(&payload);
        let inverted: Vec<bool> = levels.iter().map(|&l| !l).collect();
        let decoded = biphase_decode(variant, &inverted);
        assert_eq!(decoded.bits, payload, "{variant:?}");
        assert_eq!(decoded.violations, 0, "{variant:?}");
    }
}

#[test]
fn biphase_reports_a_half_bit_slip() {
    let zeros = vec![false; 64];
    let levels = BiphaseEncoder::new(BiphaseVariant::Mark).encode_all(&zeros);
    let slipped = biphase_decode(BiphaseVariant::Mark, &levels[1..]);
    assert_eq!(slipped.alignment, Alignment::MidBit);
    assert!(slipped.violations > 0);

    let mut rng = Rng::new(0x51ee);
    let payload = random_bits(&mut rng, 256);
    for variant in [BiphaseVariant::Mark, BiphaseVariant::Space] {
        let levels = BiphaseEncoder::new(variant).encode_all(&payload);
        assert_eq!(
            biphase_decode(variant, &levels[1..]).alignment,
            Alignment::MidBit,
            "{variant:?}"
        );
    }
}

#[test]
fn biphase_flags_a_missing_boundary_transition() {
    let zeros = vec![false; 4];
    let mut levels = BiphaseEncoder::new(BiphaseVariant::Mark).encode_all(&zeros);
    levels[2] = !levels[2];
    let decoded = biphase_decode(BiphaseVariant::Mark, &levels);
    assert_eq!(decoded.violations, 1);
    assert_eq!(decoded.bits.len(), zeros.len());
    assert!(decoded.bits[1], "the damaged pair reads as a spurious mark");
    assert_eq!(decoded.bits[2..], zeros[2..], "damage does not propagate");
}

#[test]
fn biphase_streaming_decoder_resets_the_boundary_check() {
    let mut decoder = BiphaseDecoder::new(BiphaseVariant::Mark);
    assert!(decoder.decode(true, true).boundary_transition);
    assert!(!decoder.decode(true, true).boundary_transition);
    decoder.reset();
    assert!(decoder.decode(true, true).boundary_transition);
}

#[test]
fn pair_codes_report_ambiguity_honestly() {
    for levels in [&[] as &[bool], &[true]] {
        for convention in [
            ManchesterConvention::Ieee8023,
            ManchesterConvention::GeThomas,
        ] {
            let decoded = manchester_decode(convention, levels);
            assert!(decoded.bits.is_empty());
            assert_eq!(decoded.violations, 0);
            assert_eq!(decoded.alignment, Alignment::Ambiguous);
        }
        let decoded = biphase_decode(BiphaseVariant::Mark, levels);
        assert!(decoded.bits.is_empty());
        assert_eq!(decoded.alignment, Alignment::Ambiguous);
    }

    let constant = manchester_encode(ManchesterConvention::Ieee8023, &[false; 32]);
    assert_eq!(
        manchester_decode(ManchesterConvention::Ieee8023, &constant).alignment,
        Alignment::Ambiguous
    );
}
