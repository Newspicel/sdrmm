use super::*;

#[test]
fn the_reference_map_is_the_wifi_geometry() {
    let p = OfdmParams::wifi_like();
    assert_eq!(p.fft(), 64);
    assert_eq!(p.cp(), 16);
    assert_eq!(p.data_subcarriers(), 48);
    assert_eq!(p.map().pilots().len(), 4);
    assert_eq!(p.map().occupied().len(), 52);
    let offsets: Vec<i32> = p.map().occupied().iter().map(|c| c.offset).collect();
    assert!(offsets.windows(2).all(|w| w[1] > w[0]));
    assert_eq!(offsets.first(), Some(&-26));
    assert_eq!(offsets.last(), Some(&26));
    assert!(!offsets.contains(&0));
    let dc_neighbour = p.map().occupied().iter().find(|c| c.offset == -1).unwrap();
    assert_eq!(dc_neighbour.bin, 63);
    assert_eq!(p.preamble().short_period(64), 16);
    assert_eq!(p.short_bins().len(), 12);
    assert_eq!(p.data_offset(), 10 * 16 + 32 + 2 * 64);
}

#[test]
fn the_frame_energy_is_the_geometry_and_nothing_else() {
    let p = OfdmParams::wifi_like();
    assert!((p.frame_energy(1) - (260.0 + 65.0)).abs() < 1e-9);
    assert!((p.frame_energy(64) - (260.0 + 65.0 * 64.0)).abs() < 1e-9);
    let want = 10.0 * f64::log10((260.0 + 65.0 * 64.0) / (48.0 * 64.0));
    assert!((p.framing_overhead_db(64) - want).abs() < 1e-12);
    assert!(p.framing_overhead_db(1024) < p.framing_overhead_db(64));
    assert!(p.framing_overhead_db(1_000_000) > 10.0 * (65.0f64 / 48.0).log10() - 1e-6);
}

#[test]
fn both_training_halves_carry_a_data_symbols_energy() {
    let p = OfdmParams::wifi_like();
    let long: f64 = (0..p.map().occupied().len())
        .map(|i| f64::from(p.long_training(i).norm_sqr()))
        .sum();
    let short: f64 = (0..p.short_bins().len())
        .map(|i| f64::from(p.short_training(i).norm_sqr()))
        .sum();
    assert!((long - 52.0).abs() < 1e-6, "long training energy {long}");
    assert!((short - 52.0).abs() < 1e-4, "short training energy {short}");
}

#[test]
fn the_training_sequence_follows_the_offset_not_the_order() {
    let forwards = SubcarrierMap::new(64, &[1, 2, 3, 4], &[]).unwrap();
    let backwards = SubcarrierMap::new(64, &[4, 3, 2, 1], &[]).unwrap();
    assert_eq!(forwards, backwards);
    let p = OfdmParams::wifi_like();
    let mixed: Vec<f32> = (0..p.map().occupied().len())
        .map(|i| p.long_training(i).re)
        .collect();
    assert!(mixed.iter().any(|&s| s > 0.0) && mixed.iter().any(|&s| s < 0.0));
    let positives = mixed.iter().filter(|&&s| s > 0.0).count();
    assert!((17..=35).contains(&positives), "{positives} of 52 positive");
}

#[test]
fn the_dmt_map_is_the_positive_half_and_only_it() {
    let p = OfdmParams::dmt_like();
    assert_eq!(p.domain(), Domain::RealHermitian);
    assert_eq!(p.data_subcarriers(), 24);
    assert!(
        p.map()
            .occupied()
            .iter()
            .all(|c| c.offset > 0 && c.offset < 32)
    );
    let ofdm = OfdmParams::wifi_like();
    assert!((p.frame_energy(64) - ofdm.frame_energy(64)).abs() < 1e-9);
    let cost = p.framing_overhead_db(64) - ofdm.framing_overhead_db(64);
    assert!((cost - 3.0103).abs() < 1e-3, "DMT costs {cost} dB");
}

#[test]
fn bad_parameter_sets_are_rejected_with_the_right_error() {
    assert_eq!(
        SubcarrierMap::new(48, &[1], &[]).unwrap_err(),
        OfdmError::FftSize(48)
    );
    assert_eq!(
        SubcarrierMap::new(64, &[], &[]).unwrap_err(),
        OfdmError::NoDataSubcarriers
    );
    assert_eq!(
        SubcarrierMap::new(64, &[32], &[]).unwrap_err(),
        OfdmError::SubcarrierOutOfRange {
            offset: 32,
            fft: 64
        }
    );
    assert_eq!(
        SubcarrierMap::new(64, &[3], &[3]).unwrap_err(),
        OfdmError::DuplicateSubcarrier(3)
    );
    assert_eq!(
        SubcarrierMap::new(64, &[3, -61], &[]).unwrap_err(),
        OfdmError::SubcarrierOutOfRange {
            offset: -61,
            fft: 64
        }
    );

    let map = || SubcarrierMap::new(64, &[1, 2, 3, 4], &[8]).unwrap();
    let pattern = || PilotPattern::m_sequence(vec![Complex::new(1.0, 0.0)]).unwrap();
    let preamble = Preamble {
        short_stride: 4,
        short_repeats: 10,
        long_repeats: 2,
        long_guard: 32,
    };
    let build = |cp, pilots: PilotPattern, preamble, domain| {
        OfdmParams::new(cp, map(), pilots, preamble, domain).unwrap_err()
    };
    assert_eq!(
        build(64, pattern(), preamble, Domain::Complex),
        OfdmError::CyclicPrefix { fft: 64, cp: 64 }
    );
    assert_eq!(
        build(
            16,
            PilotPattern::m_sequence(vec![Complex::new(1.0, 0.0); 2]).unwrap(),
            preamble,
            Domain::Complex
        ),
        OfdmError::PilotCountMismatch {
            pilots: 1,
            values: 2
        }
    );
    assert_eq!(
        build(
            16,
            pattern(),
            Preamble {
                short_stride: 5,
                ..preamble
            },
            Domain::Complex
        ),
        OfdmError::ShortStride { fft: 64, stride: 5 }
    );
    assert_eq!(
        build(
            16,
            pattern(),
            Preamble {
                long_repeats: 1,
                ..preamble
            },
            Domain::Complex
        ),
        OfdmError::Repeats {
            what: "long training",
            repeats: 1
        }
    );
    assert_eq!(
        build(
            16,
            pattern(),
            Preamble {
                short_stride: 16,
                ..preamble
            },
            Domain::Complex
        ),
        OfdmError::ShortTrainingEmpty
    );
    let straddling = SubcarrierMap::new(64, &[-4, 4], &[8]).unwrap();
    assert_eq!(
        OfdmParams::new(16, straddling, pattern(), preamble, Domain::RealHermitian).unwrap_err(),
        OfdmError::NotHermitian(-4)
    );
}
