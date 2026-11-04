use std::sync::Arc;

use sdrmm_wire::{
    ChannelDescriptor, EventCatalog, HzRange, Modulation, ProtocolMatch, SignalMeasure,
    SignalSignature,
};

use super::{detect::Band, features::Waveform};

const MAX_CANDIDATES: usize = 5;

const DVBT_BANDS: &[HzRange] = &[HzRange::mhz(174.0, 230.0), HzRange::mhz(470.0, 862.0)];

/// Signals Signal ID names though no decoder reads them.
fn classes() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "Unmodulated carrier",
            &[Modulation::Carrier],
            HzRange::new(0.0, 2_500.0),
            "a bare line with nothing on it",
        ),
        SignalSignature::new(
            "DVB-T (2k)",
            &[Modulation::Ofdm],
            HzRange::new(100_000.0, 8_000_000.0),
            "OFDM with a 224 µs useful symbol, terrestrial television",
        )
        .symbol_length(HzRange::about(224.0, 0.03))
        .on(DVBT_BANDS),
        SignalSignature::new(
            "DVB-T (8k)",
            &[Modulation::Ofdm],
            HzRange::new(100_000.0, 8_000_000.0),
            "OFDM with an 896 µs useful symbol, terrestrial television",
        )
        .symbol_length(HzRange::about(896.0, 0.03))
        .on(DVBT_BANDS),
    ]
}

struct Named {
    signature: SignalSignature,
    type_id: Option<String>,
}

/// The decoders and event kinds registered when a Signal ID or monitor opened, so its DSP thread
/// never reads the registry.
pub(crate) struct Catalog {
    signatures: Vec<Named>,
    descriptors: Vec<ChannelDescriptor>,
    events: Arc<EventCatalog>,
}

impl Catalog {
    pub(crate) fn registered() -> Self {
        let descriptors = crate::descriptors();
        Self::new(descriptors, sdrmm_wire::catalog())
    }

    pub(crate) fn new(descriptors: Vec<ChannelDescriptor>, events: Arc<EventCatalog>) -> Self {
        let classes = classes().into_iter().map(|signature| Named {
            signature,
            type_id: None,
        });
        let decoders = descriptors.iter().flat_map(|descriptor| {
            descriptor.signatures.iter().map(|signature| Named {
                signature: signature.clone(),
                type_id: Some(descriptor.type_id.clone()),
            })
        });
        Self {
            signatures: classes.chain(decoders).collect(),
            descriptors,
            events,
        }
    }

    pub(crate) fn descriptor(&self, type_id: &str) -> Option<&ChannelDescriptor> {
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.type_id == type_id)
    }

    pub(crate) fn events(&self) -> &EventCatalog {
        &self.events
    }

    pub(crate) fn candidates(
        &self,
        modulation: Modulation,
        band: &Band,
        waveform: &Waveform,
        frequency_hz: Option<f64>,
    ) -> Vec<ProtocolMatch> {
        let measure = SignalMeasure {
            bandwidth_hz: band.bandwidth_hz,
            symbol_rate_hz: waveform.symbol_rate_hz,
            deviation_hz: waveform.deviation_hz,
            ofdm_symbol_us: waveform.ofdm_symbol_us,
            burst_ms: waveform.burst_ms,
            frequency_hz,
        };
        let mut found: Vec<(ProtocolMatch, f64)> = self
            .signatures
            .iter()
            .filter(|named| named.signature.modulations.contains(&modulation))
            .filter_map(|named| {
                let mut matched = named.signature.matched(&measure)?;
                matched.type_id.clone_from(&named.type_id);
                Some((matched, named.signature.symbol_rate_tolerance()))
            })
            .collect();
        found.sort_by(|(a, a_tolerance), (b, b_tolerance)| {
            b.score
                .total_cmp(&a.score)
                .then(a_tolerance.total_cmp(b_tolerance))
        });
        found.truncate(MAX_CANDIDATES);
        found.into_iter().map(|(matched, _)| matched).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        Catalog::registered().candidates(modulation, band, waveform, frequency_hz)
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

    fn plugin(signatures: Vec<SignalSignature>) -> ChannelDescriptor {
        ChannelDescriptor {
            type_id: "probe_plugin".to_owned(),
            name: "Probe".to_owned(),
            signatures,
            ..ChannelDescriptor::default()
        }
    }

    fn chirp_scale() -> SignalSignature {
        SignalSignature::new(
            "Probe sweep",
            &[Modulation::Chirp],
            HzRange::new(900_000.0, 1_100_000.0),
            "a megahertz sweep",
        )
    }

    #[test]
    fn a_plugin_signature_names_its_own_decoder() {
        let catalog = Catalog::new(vec![plugin(vec![chirp_scale()])], sdrmm_wire::catalog());
        let found = catalog.candidates(
            Modulation::Chirp,
            &band(1_000_000.0),
            &Waveform::default(),
            None,
        );
        assert_eq!(names(&found), ["Probe sweep"]);
        assert_eq!(found[0].type_id.as_deref(), Some("probe_plugin"));
    }

    #[test]
    fn a_decoder_without_signatures_is_never_offered() {
        let silent = plugin(Vec::new());
        let catalog = Catalog::new(vec![silent], sdrmm_wire::catalog());
        let found = catalog.candidates(
            Modulation::Chirp,
            &band(1_000_000.0),
            &Waveform::default(),
            None,
        );
        assert!(found.is_empty(), "{found:?}");
    }
}
