#![allow(clippy::expect_used)]

//! Signal identification against the registered decoders' catalog and their signals.

//! Signal ID against the built-in registry: what each decoder declares is found on the air.

mod catalog {

    use sdrmm_decoder::signal_id::{Catalog, features::Waveform, measure};
    use sdrmm_dsp::measure::detect::Band;
    use sdrmm_wire::{Modulation, ProtocolMatch};

    fn band(bandwidth_hz: f64) -> Band {
        Band {
            center_hz: 0.0,
            bandwidth_hz,
            snr_db: 25.0,
            carrier_db: 3.0,
            flatness: 0.3,
            skew: 0.0,
            peak_hz: 0.0,
        }
    }

    fn keyed(symbol_rate_hz: f64, deviation_hz: f64) -> Waveform {
        Waveform {
            duty: 1.0,
            symbol_rate_hz: Some(symbol_rate_hz),
            deviation_hz,
            ..Waveform::default()
        }
    }

    fn candidates(
        modulation: Modulation,
        band: &Band,
        waveform: &Waveform,
        frequency_hz: Option<f64>,
    ) -> Vec<ProtocolMatch> {
        Catalog::registered().candidates(modulation, &measure(band, waveform, frequency_hz))
    }

    fn names(found: &[ProtocolMatch]) -> Vec<&str> {
        found.iter().map(|m| m.name.as_str()).collect()
    }

    #[test]
    fn a_pager_shift_finds_pocsag_at_its_own_baud() {
        let found = candidates(
            Modulation::Fsk2,
            &band(12_500.0),
            &keyed(1_200.0, 4_500.0),
            None,
        );
        assert_eq!(names(&found).first(), Some(&"POCSAG (1200 bd)"));
    }

    #[test]
    fn ermes_uses_its_symbol_rate_and_occupied_bandwidth() {
        let found = candidates(
            Modulation::Fsk4,
            &band(13_125.0),
            &keyed(3_126.0, 4_762.0),
            Some(169_650_000.0),
        );
        assert_eq!(names(&found).first(), Some(&"ERMES"));
        assert_eq!(found[0].score, 1.0);
    }

    #[test]
    fn a_maritime_carrier_finds_ais() {
        let found = candidates(
            Modulation::Fsk2,
            &band(25_000.0),
            &keyed(9_600.0, 2_400.0),
            None,
        );
        assert_eq!(names(&found).first(), Some(&"AIS"));
    }

    #[test]
    fn the_c4fm_family_comes_back_as_a_shortlist() {
        let found = candidates(
            Modulation::Fsk4,
            &band(12_500.0),
            &keyed(4_800.0, 1_944.0),
            None,
        );
        let names = names(&found);
        for mode in ["DMR", "P25 Phase 1", "System Fusion"] {
            assert!(names.contains(&mode), "{mode} missing from {names:?}");
        }
        assert!(found.iter().all(|m| !m.confirmed));
    }

    #[test]
    fn a_broadcast_signal_is_wideband_fm_and_not_a_repeater() {
        let waveform = Waveform {
            deviation_hz: 45_000.0,
            ..Waveform::default()
        };
        let found = candidates(Modulation::Fm, &band(180_000.0), &waveform, None);
        assert_eq!(names(&found).first(), Some(&"FM broadcast"));
    }

    #[test]
    fn nothing_is_offered_for_a_family_no_protocol_here_uses() {
        assert!(
            candidates(
                Modulation::Carrier,
                &band(200_000.0),
                &keyed(9_600.0, 0.0),
                None
            )
            .is_empty()
        );
    }

    #[test]
    fn the_dial_frequency_settles_a_tie_between_look_alikes() {
        let waveform = Waveform {
            deviation_hz: 2_500.0,
            ..Waveform::default()
        };
        let at_sea = candidates(
            Modulation::Fm,
            &band(12_500.0),
            &waveform,
            Some(156_800_000.0),
        );
        assert_eq!(names(&at_sea).first(), Some(&"Marine VHF voice (FM)"));
        assert!(
            at_sea[0].why.ends_with("inside its allocation"),
            "{}",
            at_sea[0].why
        );
        let on_pmr = candidates(
            Modulation::Fm,
            &band(12_500.0),
            &waveform,
            Some(446_006_250.0),
        );
        assert_eq!(names(&on_pmr).first(), Some(&"PMR446 voice (FM)"));
        let anywhere = candidates(Modulation::Fm, &band(12_500.0), &waveform, None);
        assert!(anywhere.iter().all(|m| !m.why.contains("allocation")));
    }

    #[test]
    fn a_bare_ism_frequency_does_not_make_pocsag_of_a_pager_shift_elsewhere() {
        let found = candidates(
            Modulation::Fsk2,
            &band(12_500.0),
            &keyed(1_200.0, 4_500.0),
            Some(466_075_000.0),
        );
        assert_eq!(names(&found).first(), Some(&"POCSAG (1200 bd)"));
        assert!(found[0].score > 0.99, "{}", found[0].score);
        let far = candidates(
            Modulation::Fsk2,
            &band(12_500.0),
            &keyed(1_200.0, 4_500.0),
            Some(14_000_000.0),
        );
        assert_eq!(names(&far).first(), Some(&"POCSAG (1200 bd)"));
        assert!(far[0].score < found[0].score);
    }

    #[test]
    fn a_millisecond_symbol_in_band_three_is_dab() {
        let waveform = Waveform {
            ofdm_symbol_us: Some(998.0),
            ofdm_guard_us: Some(246.0),
            ofdm_strength: 0.18,
            ..Waveform::default()
        };
        let found = candidates(
            Modulation::Ofdm,
            &band(192_000.0),
            &waveform,
            Some(227_360_000.0),
        );
        assert_eq!(names(&found).first(), Some(&"DAB / DAB+"));
    }

    #[test]
    fn slot_bursts_lift_dmr_over_the_other_c4fm_modes() {
        let bursty = Waveform {
            burst_ms: Some(29.5),
            burst_period_ms: Some(60.0),
            ..keyed(4_800.0, 1_944.0)
        };
        let found = candidates(Modulation::Fsk4, &band(12_500.0), &bursty, None);
        assert_eq!(names(&found).first(), Some(&"DMR"));
        let steady = candidates(
            Modulation::Fsk4,
            &band(12_500.0),
            &keyed(4_800.0, 1_944.0),
            None,
        );
        let dmr = |found: &[ProtocolMatch]| found.iter().find(|m| m.name == "DMR").map(|m| m.score);
        assert_eq!(
            dmr(&steady),
            dmr(&candidates(
                Modulation::Fsk4,
                &band(12_500.0),
                &keyed(4_800.0, 1_944.0),
                None
            ))
        );
    }

    #[test]
    fn short_bursts_at_the_transponder_frequency_are_mode_s() {
        let waveform = Waveform {
            burst_ms: Some(0.12),
            envelope_variation: 0.5,
            ..Waveform::default()
        };
        let found = candidates(
            Modulation::NoiseLike,
            &band(192_000.0),
            &waveform,
            Some(1_090_000_000.0),
        );
        assert_eq!(names(&found).first(), Some(&"ADS-B / Mode S"));
    }

    #[test]
    fn a_keyed_remote_on_433_mhz_offers_the_ism_decoder() {
        let found = candidates(
            Modulation::Ook,
            &band(20_000.0),
            &keyed(2_500.0, 0.0),
            Some(433_920_000.0),
        );
        let remote = found.iter().find(|m| m.name == "ISM remote (OOK)");
        assert_eq!(remote.and_then(|m| m.type_id.as_deref()), Some("ism"));
    }
}

mod chirp {

    use std::f64::consts::PI;

    use num_complex::Complex;
    use sdrmm_decoder::signal_id::{
        Catalog, Identifier,
        chirp::{Chirp, Chirps},
        features,
    };
    use sdrmm_dsp::measure::detect::Band;
    use sdrmm_wire::Modulation;

    use sdrmm_channels::synth::lora::LoraWaveform;
    use sdrmm_decoder_test_support::{cf32_le, complex_noise};

    fn band(bandwidth_hz: f64) -> Band {
        Band {
            center_hz: 0.0,
            bandwidth_hz,
            snr_db: 10.0,
            carrier_db: 0.0,
            flatness: 0.9,
            skew: 0.0,
            peak_hz: 0.0,
        }
    }

    fn chirp_of(iq: &[Complex<f32>], rate: f64, occupied_hz: f64) -> Option<Chirp> {
        let band = band(occupied_hz);
        let zoom = features::zoom(iq, rate, &band)?;
        Chirps::default().find(&zoom, &band, Catalog::registered().chirps())
    }

    #[test]
    fn a_real_rn2483_frame_reads_as_sf7_at_125_khz() {
        let iq = cf32_le(include_bytes!(
            "../../../../fixtures/lora_rn2483_sf7_250k.sigmf-data"
        ));
        let chirp = chirp_of(&iq, 250_000.0, 118_000.0).expect("chirps");
        assert_eq!(chirp.spreading_factor, 7);
        assert_eq!(chirp.bandwidth_hz, 125_000.0);
        assert!(!chirp.inverted);
    }

    #[test]
    fn identify_names_a_real_lora_frame_and_offers_the_lora_decoder() {
        let iq = cf32_le(include_bytes!(
            "../../../../fixtures/lora_rn2483_sf7_250k.sigmf-data"
        ));
        let band = band(118_000.0);
        let signal = Identifier::new().identify(
            &iq,
            250_000.0,
            &band,
            868_100_000.0,
            &Catalog::registered(),
        );
        assert_eq!(signal.modulation, Modulation::Chirp);
        assert_eq!(signal.bandwidth_hz, 125_000.0);
        assert_eq!(signal.symbol_rate_hz, Some(125_000.0 / 128.0));
        assert_eq!(
            signal.best().and_then(|best| best.type_id.as_deref()),
            Some("lora")
        );
    }

    #[test]
    fn a_weak_sf10_frame_is_found_under_the_noise() {
        let waveform = LoraWaveform {
            spreading_factor: 10,
            bandwidth_hz: 125_000.0,
            ..LoraWaveform::default()
        };
        let rate = 500_000.0;
        let mut iq = waveform.frame_at(&[0x42; 24], rate, 0.0);
        let noise = complex_noise(7, 1.8, iq.len());
        for (sample, noise) in iq.iter_mut().zip(noise) {
            *sample += noise;
        }
        let chirp = chirp_of(&iq, rate, 110_000.0).expect("chirps");
        assert_eq!(chirp.spreading_factor, 10);
        assert_eq!(chirp.bandwidth_hz, 125_000.0);
    }

    #[test]
    fn inverted_iq_reads_as_downchirps() {
        let waveform = LoraWaveform {
            inverted: true,
            ..LoraWaveform::default()
        };
        let rate = 500_000.0;
        let iq = waveform.frame_at(&[7; 16], rate, 0.0);
        let chirp = chirp_of(&iq, rate, 120_000.0).expect("chirps");
        assert!(chirp.inverted);
    }

    #[test]
    fn noise_and_fsk_are_not_chirps() {
        let rate = 500_000.0;
        assert!(chirp_of(&complex_noise(3, 1.0, 200_000), rate, 120_000.0).is_none());
        let fsk: Vec<Complex<f32>> = (0..200_000)
            .map(|n| {
                let bit = (n / 25) % 3 == 0;
                let shift = if bit { 40_000.0 } else { -40_000.0 };
                Complex::from_polar(1.0, (2.0 * PI * shift * f64::from(n) / rate) as f32)
            })
            .collect();
        assert!(chirp_of(&fsk, rate, 120_000.0).is_none());
    }
}

mod confirm {

    use num_complex::Complex;
    use sdrmm_decoder::signal_id::{
        Catalog,
        confirm::{Confirmer, confirmable, decoded},
    };
    use sdrmm_dsp::measure::detect::Band;
    use sdrmm_wire::{DecoderEvent, ProtocolMatch};

    fn band() -> Band {
        Band {
            center_hz: 0.0,
            bandwidth_hz: 12_500.0,
            snr_db: 20.0,
            carrier_db: 12.0,
            flatness: 0.5,
            skew: 0.0,
            peak_hz: 0.0,
        }
    }

    fn pocsag() -> ProtocolMatch {
        ProtocolMatch {
            name: "POCSAG".to_owned(),
            type_id: Some("pocsag".to_owned()),
            score: 0.5,
            confirmed: false,
            why: String::new(),
        }
    }

    fn dsc(crc_ok: bool) -> DecoderEvent {
        DecoderEvent::tagged(
            sdrmm_wire::EventKind::named("dsc"),
            &sdrmm_decoder::link::DataLinkMessage::<sdrmm_decoder::link::NoRegion> {
                message_type: "Distress".to_owned(),
                station: None,
                text: None,
                crc_ok,
                fec_corrected: None,
                snr_db: None,
                frequency_error_hz: None,
                lat: None,
                lon: None,
                raw: None,
                area: None,
                details: serde_json::Value::Null,
            },
        )
    }

    #[test]
    fn only_a_frame_passing_its_check_proves_a_trial() {
        let catalog = Catalog::registered();
        let descriptor = catalog.descriptor("dsc").expect("dsc is registered");
        assert!(decoded(descriptor, &catalog, &dsc(true)));
        assert!(!decoded(descriptor, &catalog, &dsc(false)));
        let nfm = catalog.descriptor("nfm").expect("nfm is registered");
        assert!(!confirmable(&catalog, &nfm.type_id));
    }

    #[test]
    fn a_decoder_that_will_not_start_is_explained_and_not_tried_again() {
        let mut confirmer = Confirmer::new();
        let catalog = Catalog::registered();
        let mut candidates = vec![pocsag()];
        let iq = vec![Complex::new(0.0, 0.0); 4_096];
        let unusable_rate = f64::NAN;

        confirmer.confirm(
            &mut candidates,
            &catalog,
            &iq,
            unusable_rate,
            &band(),
            148_000_000.0,
        );
        assert!(
            candidates[0].why.contains("will not start"),
            "nothing said why: {:?}",
            candidates[0].why
        );
        assert_eq!(confirmer.refused(), 1);

        confirmer.sweep();
        candidates[0].why.clear();
        confirmer.confirm(
            &mut candidates,
            &catalog,
            &iq,
            unusable_rate,
            &band(),
            148_000_000.0,
        );
        assert_eq!(confirmer.refused(), 1, "the decoder was started again");
        assert!(
            !candidates[0].why.is_empty(),
            "the refusal went unreported on the second window"
        );
    }
}

mod framing {

    use num_complex::Complex;
    use sdrmm_decoder::{
        catalog::SyncSearch,
        signal_id::{
            Catalog,
            framing::{confirm, hits},
        },
    };
    use sdrmm_dsp::measure::detect::Band;
    use sdrmm_wire::ProtocolMatch;

    fn catalog() -> Catalog {
        Catalog::registered()
    }

    fn mode<'a>(catalog: &'a Catalog, type_id: &str) -> &'a SyncSearch {
        catalog
            .syncs()
            .iter()
            .find(|mode| mode.type_id == type_id)
            .expect("the mode is one of the searched signatures")
    }

    fn sync_symbols(mode: &SyncSearch, pattern: u64) -> Vec<f32> {
        let mapping = mode.params.mapping();
        let per_symbol = mapping.bits_per_symbol();
        (0..mode.sync.bits / per_symbol)
            .map(|symbol| {
                let shift = mode.sync.bits - (symbol + 1) * per_symbol;
                let index = (pattern >> shift) & ((1u64 << per_symbol) - 1);
                mapping.level(index as u8)
            })
            .collect()
    }

    fn band() -> Band {
        Band {
            center_hz: 0.0,
            bandwidth_hz: 12_500.0,
            snr_db: 20.0,
            carrier_db: 12.0,
            flatness: 0.5,
            skew: 0.0,
            peak_hz: 0.0,
        }
    }

    #[test]
    fn a_sync_word_at_the_very_start_of_the_window_is_found() {
        let catalog = catalog();
        let dmr = mode(&catalog, "dmr");
        let soft = sync_symbols(dmr, dmr.sync.patterns[0]);
        assert_eq!(
            hits(&soft, dmr),
            1,
            "the sync word filling the register was skipped"
        );
    }

    #[test]
    fn a_frame_search_that_cannot_run_says_why() {
        let catalog = catalog();
        let mut candidates = vec![ProtocolMatch {
            name: mode(&catalog, "dmr").name.clone(),
            type_id: Some("dmr".to_owned()),
            score: 0.5,
            confirmed: false,
            why: String::new(),
        }];
        let iq = vec![Complex::new(0.0, 0.0); 4_096];
        confirm(&catalog, &mut candidates, &iq, f64::NAN, &band());
        assert!(
            candidates[0].why.contains("frame search"),
            "nothing said why: {:?}",
            candidates[0].why
        );
    }
}

mod features {
    use sdrmm_decoder::signal_id::features::{Meter, ZOOM_OVERSAMPLE, Zoom};
    use sdrmm_dsp::measure::detect::Band;

    const RATE: f64 = 48_000.0;

    fn band() -> Band {
        Band {
            center_hz: 0.0,
            bandwidth_hz: RATE / ZOOM_OVERSAMPLE,
            snr_db: 40.0,
            carrier_db: 3.0,
            flatness: 0.3,
            skew: 0.0,
            peak_hz: 0.0,
        }
    }

    #[test]
    fn an_acars_burst_reads_its_clock_and_not_the_octave_above_it() {
        let block = sdrmm_channels::synth::acars::Block {
            mode: '2',
            registration: ".D-AIBC",
            ack: '\x15',
            label: "H1",
            block_id: '3',
            seq_no: Some("M01A"),
            flight: Some("LH0400"),
            text: "SDRmm FIXTURE",
            more: false,
        };
        let iq = sdrmm_channels::synth::acars::transmission(&block, RATE);
        let baud = Meter::new()
            .measure(&Zoom { rate: RATE, iq }, &band())
            .symbol_rate_hz
            .expect("a keyed carrier has a symbol clock");
        assert!((baud - 2_400.0).abs() < 120.0, "baud {baud}");
    }
}
