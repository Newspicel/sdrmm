use num_complex::Complex;
use sdrmm_dsp::{Ddc, RealDecimator, design_lowpass};
use sdrmm_wire::{ChannelDescriptor, DecoderEvent, IdentSignal, SpectrumMonitorNode};

use crate::ident::Catalog;
use crate::{
    ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, audio_channels,
    channel_filter, create,
};

const MIN_ANALOG_SCORE: f32 = 0.7;

pub(super) struct Decoder {
    pub(super) kind: String,
    pub(super) verified: bool,
    ddc: Ddc,
    tuning_offset_hz: f64,
    filter: ChannelFilter,
    channel: Box<dyn ChannelRx>,
    tuned: Vec<Complex<f32>>,
    filtered: Vec<Complex<f32>>,
    out: ChannelOutputs,
    channels: usize,
    decimator: RealDecimator,
    mono: Vec<f32>,
    downsampled: Vec<f32>,
    pub(super) audio: Vec<i16>,
    record: bool,
    frequency_hz: f64,
}

pub(super) fn choices(
    signal: &IdentSignal,
    settings: &SpectrumMonitorNode,
    catalog: &Catalog,
) -> Vec<String> {
    let analog = analog(signal, catalog);
    let mut choices = Vec::new();
    for candidate in &signal.candidates {
        if let Some(kind) = &candidate.type_id
            && let Some(descriptor) = catalog.descriptor(kind)
            && descriptor.identifiable
            && (!descriptor.analog_voice || analog == Some(kind.as_str()))
            && !choices.contains(kind)
        {
            choices.push(kind.clone());
        }
    }
    if let Some(kind) = analog
        && !choices.iter().any(|choice| choice == kind)
    {
        choices.push(kind.to_owned());
    }
    choices.retain(|kind| settings.decodes(kind));
    choices
}

/// The analog voice decoder for `signal`: the best candidate that is one, heard in a
/// modulation its signatures name.
fn analog<'a>(signal: &'a IdentSignal, catalog: &Catalog) -> Option<&'a str> {
    signal.candidates.iter().find_map(|candidate| {
        let kind = candidate.type_id.as_deref()?;
        let descriptor = catalog.descriptor(kind)?;
        (candidate.score >= MIN_ANALOG_SCORE && speaks(descriptor, signal)).then_some(kind)
    })
}

fn speaks(descriptor: &ChannelDescriptor, signal: &IdentSignal) -> bool {
    descriptor.analog_voice
        && descriptor
            .signatures
            .iter()
            .any(|signature| signature.modulations.contains(&signal.modulation))
}

impl Decoder {
    pub(super) fn new(
        kind: &str,
        rate: f64,
        signal: &IdentSignal,
        record: bool,
        catalog: &Catalog,
    ) -> Result<Self, ChannelError> {
        let mut settings = catalog
            .descriptor(kind)
            .and_then(|descriptor| descriptor.defaults.clone())
            .ok_or_else(|| ChannelError::UnknownType(kind.to_owned()))?;
        settings.frequency_hz = signal.frequency_hz;
        settings.squelch = sdrmm_wire::Squelch::Off;
        let tuned = crate::tune(&settings.params, signal)?;
        settings.params = tuned.params;
        let offset = signal.center_offset_hz + tuned.offset_hz;
        let input_rate = crate::input_rate(&settings.params);
        Ok(Self {
            kind: kind.to_owned(),
            verified: false,
            tuning_offset_hz: offset - signal.center_offset_hz,
            ddc: Ddc::new(rate, input_rate, offset)
                .map_err(|e| ChannelError::InvalidSettings(e.to_string()))?,
            filter: channel_filter(&settings.params)?,
            channels: usize::from(audio_channels(&settings.params)),
            channel: create(ChannelCtx { input_rate }, &settings)?,
            tuned: Vec::new(),
            filtered: Vec::new(),
            out: ChannelOutputs::default(),
            decimator: RealDecimator::new(&design_lowpass(96, 3400.0 / 48_000.0), 6),
            mono: Vec::new(),
            downsampled: Vec::new(),
            audio: Vec::new(),
            record,
            frequency_hz: signal.frequency_hz,
        })
    }

    pub(super) fn retune(&mut self, offset_hz: f64) {
        self.ddc.set_offset(offset_hz + self.tuning_offset_hz);
    }

    pub(super) fn selected(&self, signal: &IdentSignal, catalog: &Catalog) -> bool {
        self.verified || analog(signal, catalog) == Some(self.kind.as_str())
    }

    pub(super) fn process(
        &mut self,
        iq: &[Complex<f32>],
        catalog: &Catalog,
    ) -> (Vec<DecoderEvent>, Option<String>) {
        self.ddc.process(iq, &mut self.tuned);
        self.filter.process(&self.tuned, &mut self.filtered);
        self.out.reset();
        self.channel.process(&self.filtered, &mut self.out);
        let mut error = None;
        if !self.out.images.is_empty() || !self.out.video.is_empty() {
            error = Some("decoded images or video are not supported by event export".to_owned());
        }
        let descriptor = catalog.descriptor(&self.kind);
        self.out
            .events
            .retain(|event| valid(event, descriptor, self.frequency_hz, catalog));
        self.verified |= self.out.events.iter().any(|event| verifies(event, catalog));
        if self.record && !self.out.audio_pcm.is_empty() {
            if self.out.audio_rate != 48_000 {
                error = Some(format!(
                    "unsupported decoder audio rate {}",
                    self.out.audio_rate
                ));
            } else {
                self.mono.clear();
                self.mono.extend(
                    self.out
                        .audio_pcm
                        .chunks_exact(self.channels)
                        .map(|frame| frame.iter().sum::<f32>() / self.channels as f32),
                );
                self.decimator.process(&self.mono, &mut self.downsampled);
                let room = (8000 * 31usize).saturating_sub(self.audio.len());
                if self.downsampled.len() > room {
                    error = Some("audio buffer limit reached".to_owned());
                }
                self.audio.extend(
                    self.downsampled
                        .iter()
                        .take(room)
                        .map(|sample| (sample.clamp(-1.0, 1.0) * 32767.0) as i16),
                );
            }
        }
        (std::mem::take(&mut self.out.events), error)
    }
}

fn verifies(event: &DecoderEvent, catalog: &Catalog) -> bool {
    catalog
        .events()
        .entry(&event.kind)
        .is_none_or(|entry| entry.verifies)
}

/// `event` stands as a decode: it does not fail its own check, and a reading that measures any
/// carrier comes from inside its decoder's allocation.
fn valid(
    event: &DecoderEvent,
    descriptor: Option<&ChannelDescriptor>,
    frequency_hz: f64,
    catalog: &Catalog,
) -> bool {
    let events = catalog.events();
    if events.present(event).verified == Some(false) {
        return false;
    }
    let allocated = events
        .entry(&event.kind)
        .is_some_and(|entry| entry.allocated);
    !allocated
        || descriptor.is_some_and(|descriptor| {
            descriptor
                .signatures
                .iter()
                .any(|signature| signature.in_allocation(frequency_hz))
        })
}

#[cfg(test)]
mod tests {
    use crate::ils::IlsComponent;
    use crate::ils::IlsReading;
    use crate::morse::MorseText;
    use crate::nfm::ToneSquelchStatus;
    use sdrmm_wire::{Modulation, ProtocolMatch};

    use super::*;

    #[test]
    fn weak_voice_candidates_do_not_select_analog_audio() {
        let catalog = Catalog::registered();
        let settings = SpectrumMonitorNode::default();
        let mut signal = IdentSignal {
            modulation: Modulation::Fm,
            bandwidth_hz: 7_000.0,
            confidence: 0.97,
            candidates: vec![ProtocolMatch {
                name: "FM voice (narrowband)".to_owned(),
                type_id: Some("nfm".to_owned()),
                score: 0.48,
                confirmed: false,
                why: String::new(),
            }],
            ..Default::default()
        };
        assert!(choices(&signal, &settings, &catalog).is_empty());
        signal.candidates[0].score = 0.95;
        assert_eq!(choices(&signal, &settings, &catalog), ["nfm"]);
        signal.modulation = Modulation::Fsk4;
        assert!(choices(&signal, &settings, &catalog).is_empty());
    }

    #[test]
    fn ils_measurements_outside_its_allocation_are_rejected() {
        let catalog = Catalog::registered();
        let ils = catalog.descriptor("ils");
        let event = DecoderEvent::of(&IlsReading {
            component: IlsComponent::Localizer,
            modulation_90: 0.2,
            modulation_150: 0.2,
            ddm: 0.0,
            deviation_dots: 0.0,
            signal_db: -20.0,
        });
        assert!(!valid(&event, ils, 435_125_000.0, &catalog));
        assert!(valid(&event, ils, 110_300_000.0, &catalog));
    }

    #[test]
    fn a_squelch_tone_is_not_a_decode() {
        let catalog = Catalog::registered();
        let event = DecoderEvent::of(&ToneSquelchStatus {
            ctcss_hz: Some(88.5),
            dcs_code: None,
            open: true,
        });
        assert!(!valid(
            &event,
            catalog.descriptor("nfm"),
            145_500_000.0,
            &catalog
        ));
    }

    #[test]
    fn a_sideband_signal_tunes_to_its_carrier() {
        let catalog = Catalog::registered();
        let settings = catalog
            .descriptor("ssb")
            .and_then(|descriptor| descriptor.defaults.clone())
            .expect("ssb defaults");
        let signal = IdentSignal {
            modulation: Modulation::Ssb,
            sideband: Some(sdrmm_wire::Sideband::Lsb),
            bandwidth_hz: 2_400.0,
            ..Default::default()
        };
        let tuned = crate::tune(&settings.params, &signal).expect("ssb tunes");
        assert_eq!(tuned.offset_hz, 1_200.0);
        let params: crate::ssb::SsbParams = tuned.params.parse().expect("ssb settings");
        assert_eq!(params.sideband, sdrmm_wire::Sideband::Lsb);
    }

    #[test]
    fn unframed_text_does_not_stop_decoder_trials() {
        let catalog = Catalog::registered();
        let text = DecoderEvent::of(&MorseText {
            text: "**".to_owned(),
            wpm: 79.0,
        });
        assert!(!verifies(&text, &catalog));
    }
}
