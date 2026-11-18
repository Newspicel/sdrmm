//! The decoders and signal classes Signal ID and the spectrum monitor match a measured signal
//! against.

use std::sync::Arc;

use sdrmm_modem::{
    cpm::{CpmParams, Mapping, c4fm::c4fm_params},
    pulse::{self, Norm},
};
use sdrmm_wire::{
    ChannelDescriptor, ChirpSweep, EventCatalog, FrameSync, HzRange, Modulation, ProtocolMatch,
    SignalMeasure, SignalSignature, SyncPulse,
};

const MAX_CANDIDATES: usize = 5;

/// The rate Signal ID searches a keyed signal's symbols for frame sync words at.
pub const SYNC_SEARCH_RATE_HZ: f64 = 48_000.0;

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

/// A decoder's frame sync, with the demodulator that finds it built at
/// [`SYNC_SEARCH_RATE_HZ`].
pub struct SyncSearch {
    pub name: String,
    pub type_id: String,
    pub sync: FrameSync,
    pub params: CpmParams,
    pub receive_filter: Vec<f32>,
}

impl SyncSearch {
    fn new(name: &str, type_id: &str, sync: &FrameSync) -> Self {
        let (params, receive_filter) = waveform(sync);
        Self {
            name: name.to_owned(),
            type_id: type_id.to_owned(),
            sync: sync.clone(),
            params,
            receive_filter,
        }
    }

    /// Searches that one demodulator serves alike.
    #[must_use]
    pub fn same_waveform(&self, other: &Self) -> bool {
        self.sync.symbol_rate_hz == other.sync.symbol_rate_hz
            && self.sync.deviation_hz == other.sync.deviation_hz
            && self.receive_filter == other.receive_filter
    }
}

fn waveform(sync: &FrameSync) -> (CpmParams, Vec<f32>) {
    let sps = SYNC_SEARCH_RATE_HZ / sync.symbol_rate_hz;
    match sync.pulse {
        SyncPulse::C4fm { alpha } => {
            let params = c4fm_params(
                SYNC_SEARCH_RATE_HZ,
                sync.symbol_rate_hz,
                sync.deviation_hz,
                alpha,
            );
            let receive_filter = params.freq_pulse().to_vec();
            (params, receive_filter)
        }
        SyncPulse::Gaussian {
            bt,
            pulse_span,
            matched_span,
        } => (
            CpmParams::from_deviation(
                Mapping::natural(2),
                sync.deviation_hz,
                sync.symbol_rate_hz,
                pulse::gaussian_freq(sps, bt, pulse_span, Norm::Area),
                sps,
            ),
            pulse::gaussian(sps, bt, matched_span, Norm::Area),
        ),
    }
}

/// The decoders and event kinds registered when a Signal ID or monitor opened, so its DSP thread
/// never reads the registry.
pub struct Catalog {
    signatures: Vec<Named>,
    syncs: Vec<SyncSearch>,
    chirps: Vec<ChirpSweep>,
    descriptors: Vec<ChannelDescriptor>,
    events: Arc<EventCatalog>,
}

impl Catalog {
    pub fn registered() -> Self {
        let descriptors = crate::descriptors();
        Self::new(descriptors, sdrmm_wire::catalog())
    }

    pub fn new(descriptors: Vec<ChannelDescriptor>, events: Arc<EventCatalog>) -> Self {
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
        let syncs = descriptors
            .iter()
            .flat_map(|descriptor| {
                descriptor.signatures.iter().filter_map(|signature| {
                    let sync = signature.sync.as_ref()?;
                    Some(SyncSearch::new(&signature.name, &descriptor.type_id, sync))
                })
            })
            .collect();
        let chirps = descriptors
            .iter()
            .flat_map(|descriptor| &descriptor.signatures)
            .filter_map(|signature| signature.chirp.clone())
            .collect();
        Self {
            signatures: classes.chain(decoders).collect(),
            syncs,
            chirps,
            descriptors,
            events,
        }
    }

    /// Every frame sync a registered decoder declares.
    #[must_use]
    pub fn syncs(&self) -> &[SyncSearch] {
        &self.syncs
    }

    /// Every chirp sweep a registered decoder declares.
    #[must_use]
    pub fn chirps(&self) -> &[ChirpSweep] {
        &self.chirps
    }

    pub fn descriptor(&self, type_id: &str) -> Option<&ChannelDescriptor> {
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.type_id == type_id)
    }

    pub fn events(&self) -> &EventCatalog {
        &self.events
    }

    /// The best few signatures `measure` fits under `modulation`, decoders and classes alike.
    #[must_use]
    pub fn candidates(
        &self,
        modulation: Modulation,
        measure: &SignalMeasure,
    ) -> Vec<ProtocolMatch> {
        let mut best = Best::default();
        for (index, named) in self.signatures.iter().enumerate() {
            if !named.signature.modulations.contains(&modulation) {
                continue;
            }
            if let Some((score, _)) = named.signature.score(measure) {
                best.offer(Ranked {
                    index,
                    score,
                    tolerance: named.signature.symbol_rate_tolerance(),
                });
            }
        }
        best.ranked()
            .filter_map(|ranked| {
                let named = &self.signatures[ranked.index];
                let mut matched = named.signature.matched(measure)?;
                matched.type_id.clone_from(&named.type_id);
                Some(matched)
            })
            .collect()
    }
}

#[derive(Clone, Copy)]
struct Ranked {
    index: usize,
    score: f32,
    tolerance: f64,
}

impl Ranked {
    /// Ahead on score, a tighter symbol rate breaking a tie; an earlier signature stays ahead
    /// of an equal later one.
    fn ahead_of(&self, other: &Self) -> bool {
        self.score
            .total_cmp(&other.score)
            .then(other.tolerance.total_cmp(&self.tolerance))
            .is_gt()
    }
}

/// The few best signatures seen so far, best first, kept without allocating.
#[derive(Default)]
struct Best {
    kept: [Option<Ranked>; MAX_CANDIDATES],
    len: usize,
}

impl Best {
    fn offer(&mut self, ranked: Ranked) {
        let at = self.kept[..self.len]
            .iter()
            .flatten()
            .position(|kept| ranked.ahead_of(kept))
            .unwrap_or(self.len);
        if at >= MAX_CANDIDATES {
            return;
        }
        self.kept[at..].rotate_right(1);
        self.kept[at] = Some(ranked);
        self.len = (self.len + 1).min(MAX_CANDIDATES);
    }

    fn ranked(&self) -> impl Iterator<Item = &Ranked> {
        self.kept[..self.len].iter().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn sweep() -> SignalMeasure {
        SignalMeasure {
            bandwidth_hz: 1_000_000.0,
            ..SignalMeasure::default()
        }
    }

    #[test]
    fn a_plugin_signature_names_its_own_decoder() {
        let catalog = Catalog::new(vec![plugin(vec![chirp_scale()])], sdrmm_wire::catalog());
        let found = catalog.candidates(Modulation::Chirp, &sweep());
        let names: Vec<_> = found.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Probe sweep"]);
        assert_eq!(found[0].type_id.as_deref(), Some("probe_plugin"));
    }

    #[test]
    fn a_declared_frame_sync_is_searchable() {
        let sync = FrameSync {
            symbol_rate_hz: 4_800.0,
            deviation_hz: 1_200.0,
            pulse: SyncPulse::Gaussian {
                bt: 0.5,
                pulse_span: 3,
                matched_span: 3,
            },
            bits: 24,
            tolerance: 1,
            patterns: vec![0x00AA_B468],
            min_hits: 2,
        };
        let catalog = Catalog::new(
            vec![plugin(vec![SignalSignature::synced(
                "Probe voice",
                6_250.0,
                sync,
            )])],
            sdrmm_wire::catalog(),
        );
        let [search] = catalog.syncs() else {
            panic!("one sync expected");
        };
        assert_eq!(search.type_id, "probe_plugin");
        assert_eq!(search.params.mapping().m(), 2);
        assert!((search.params.sps() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn a_decoder_without_signatures_is_never_offered() {
        let catalog = Catalog::new(vec![plugin(Vec::new())], sdrmm_wire::catalog());
        assert!(catalog.candidates(Modulation::Chirp, &sweep()).is_empty());
    }
}
