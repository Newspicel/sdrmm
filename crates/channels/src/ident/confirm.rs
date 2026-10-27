use num_complex::Complex;
use sdrmm_dsp::Ddc;
use sdrmm_wire::{
    ChannelParams, ChannelSettings, DecoderEvent, ProtocolMatch, SelcallParams, SelcallSystem,
    UNKNOWN_MODEL,
};

use super::detect::Band;
use crate::{
    ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, channel_filter, create,
    descriptor_of,
};

const MAX_TRIALS: usize = 3;

const MAX_LIVE: usize = 12;

const DEMOTION: f32 = 0.4;

const SAME_CENTRE: f64 = 0.5;

fn decoded(type_id: &str, event: &DecoderEvent) -> bool {
    match (type_id, event) {
        ("pocsag", DecoderEvent::Pocsag(_))
        | ("flex", DecoderEvent::Flex(_))
        | ("ermes", DecoderEvent::Ermes(_))
        | ("eot", DecoderEvent::Eot(_))
        | ("same", DecoderEvent::Same(_))
        | ("sarsat", DecoderEvent::Sarsat(_))
        | ("tetrapol", DecoderEvent::Tetrapol(_))
        | ("wmbus", DecoderEvent::Wmbus(_))
        | ("ais", DecoderEvent::Ais(_))
        | ("acars", DecoderEvent::Acars(_))
        | ("aprs", DecoderEvent::Aprs(_))
        | ("wfm", DecoderEvent::Rds(_))
        | ("selcall", DecoderEvent::Selcall(_)) => true,
        ("dsc", DecoderEvent::Dsc(message))
        | ("vdl2", DecoderEvent::Vdl2(message))
        | ("hfdl", DecoderEvent::Hfdl(message))
        | ("inmarsat_stdc", DecoderEvent::InmarsatStdc(message))
        | ("inmarsat_aero", DecoderEvent::InmarsatAero(message)) => message.crc_ok,
        ("ism", DecoderEvent::Ism(message)) => message.model != UNKNOWN_MODEL,
        _ => false,
    }
}

fn confirmable(type_id: &str) -> bool {
    matches!(
        type_id,
        "pocsag"
            | "flex"
            | "ermes"
            | "eot"
            | "same"
            | "sarsat"
            | "tetrapol"
            | "wmbus"
            | "ais"
            | "acars"
            | "aprs"
            | "wfm"
            | "selcall"
            | "dsc"
            | "vdl2"
            | "hfdl"
            | "inmarsat_stdc"
            | "inmarsat_aero"
            | "ism"
    )
}

struct Live {
    type_id: String,
    center_hz: f64,
    bandwidth_hz: f64,
    ddc: Ddc,
    filter: ChannelFilter,
    channel: Box<dyn ChannelRx>,
    seen: bool,
}

struct Refused {
    type_id: String,
    why: String,
}

pub(crate) struct Confirmer {
    live: Vec<Live>,
    refused: Vec<Refused>,
    tuned: Vec<Complex<f32>>,
    filtered: Vec<Complex<f32>>,
    out: ChannelOutputs,
}

impl Confirmer {
    pub(crate) fn new() -> Self {
        Self {
            live: Vec::new(),
            refused: Vec::new(),
            tuned: Vec::new(),
            filtered: Vec::new(),
            out: ChannelOutputs::default(),
        }
    }

    pub(crate) fn forget(&mut self) {
        self.live.clear();
        self.refused.clear();
    }

    pub(crate) fn sweep(&mut self) {
        self.live.retain(|live| live.seen);
        for live in &mut self.live {
            live.seen = false;
        }
    }

    pub(crate) fn confirm(
        &mut self,
        candidates: &mut Vec<ProtocolMatch>,
        iq: &[Complex<f32>],
        rate: f64,
        band: &Band,
        frequency_hz: f64,
    ) {
        let mut confirmed_any = self.feed_live(candidates, iq, band);
        let mut trials = 0;
        for candidate in candidates.iter_mut() {
            if trials >= MAX_TRIALS {
                break;
            }
            let Some(type_id) = candidate.type_id.clone().filter(|t| confirmable(t)) else {
                continue;
            };
            if candidate.confirmed || self.is_live(&type_id, band) {
                continue;
            }
            if let Some(why) = self.refusal(&type_id) {
                candidate.why = why.to_owned();
                continue;
            }
            trials += 1;
            for settings in variants(&type_id, frequency_hz) {
                if self.live.len() >= MAX_LIVE {
                    break;
                }
                let live = match start(&type_id, settings, rate, band) {
                    Ok(live) => live,
                    Err(error) => {
                        self.refuse(&type_id, candidate, &error);
                        break;
                    }
                };
                self.live.push(live);
                let last = self.live.len() - 1;
                let frames = self.run(last, iq, band);
                if frames > 0 {
                    mark(candidate, frames);
                    confirmed_any = true;
                }
            }
        }
        if confirmed_any {
            for candidate in candidates.iter_mut() {
                let rival = candidate.type_id.as_deref().is_some_and(confirmable);
                if rival && !candidate.confirmed {
                    candidate.score *= DEMOTION;
                }
            }
            candidates.sort_by(|a, b| {
                b.confirmed
                    .cmp(&a.confirmed)
                    .then(b.score.total_cmp(&a.score))
            });
        }
    }

    fn feed_live(
        &mut self,
        candidates: &mut Vec<ProtocolMatch>,
        iq: &[Complex<f32>],
        band: &Band,
    ) -> bool {
        let mut confirmed_any = false;
        for index in 0..self.live.len() {
            if self.live[index].seen || !same_signal(&self.live[index], band) {
                continue;
            }
            let frames = self.run(index, iq, band);
            if frames == 0 {
                continue;
            }
            let type_id = self.live[index].type_id.clone();
            match candidates
                .iter_mut()
                .find(|c| c.type_id.as_deref() == Some(type_id.as_str()))
            {
                Some(candidate) => mark(candidate, frames),
                None => {
                    if let Some(descriptor) = descriptor_of(&type_id) {
                        let mut candidate = ProtocolMatch {
                            name: descriptor.name.clone(),
                            type_id: Some(type_id),
                            score: 1.0,
                            confirmed: false,
                            why: String::new(),
                        };
                        mark(&mut candidate, frames);
                        candidates.push(candidate);
                    }
                }
            }
            confirmed_any = true;
        }
        confirmed_any
    }

    fn refusal(&self, type_id: &str) -> Option<&str> {
        self.refused
            .iter()
            .find(|refused| refused.type_id == type_id)
            .map(|refused| refused.why.as_str())
    }

    fn refuse(&mut self, type_id: &str, candidate: &mut ProtocolMatch, error: &ChannelError) {
        let why = format!("the {} decoder will not start: {error}", candidate.name);
        candidate.why = why.clone();
        self.refused.push(Refused {
            type_id: type_id.to_owned(),
            why,
        });
    }

    fn is_live(&self, type_id: &str, band: &Band) -> bool {
        self.live
            .iter()
            .any(|live| live.type_id == type_id && same_signal(live, band))
    }

    fn run(&mut self, index: usize, iq: &[Complex<f32>], band: &Band) -> usize {
        let live = &mut self.live[index];
        live.seen = true;
        live.center_hz = band.center_hz;
        live.bandwidth_hz = band.bandwidth_hz;
        live.ddc.process(iq, &mut self.tuned);
        live.filter.process(&self.tuned, &mut self.filtered);
        self.out.reset();
        live.channel.process(&self.filtered, &mut self.out);
        let type_id = live.type_id.as_str();
        self.out
            .events
            .iter()
            .filter(|event| decoded(type_id, event))
            .count()
    }
}

fn mark(candidate: &mut ProtocolMatch, frames: usize) {
    candidate.confirmed = true;
    candidate.score = 1.0;
    candidate.why = format!(
        "the {} decoder read {frames} frame{} from the signal",
        candidate.name,
        if frames == 1 { "" } else { "s" }
    );
}

fn same_signal(live: &Live, band: &Band) -> bool {
    let narrower = live.bandwidth_hz.min(band.bandwidth_hz).max(1.0);
    (live.center_hz - band.center_hz).abs() <= narrower * SAME_CENTRE
}

fn variants(type_id: &str, frequency_hz: f64) -> Vec<ChannelSettings> {
    let Some(mut settings) = ChannelSettings::default_for(type_id) else {
        return Vec::new();
    };
    settings.frequency_hz = frequency_hz;
    if type_id != "selcall" {
        return vec![settings];
    }
    [SelcallSystem::Ccir1, SelcallSystem::Zvei1]
        .into_iter()
        .map(|system| ChannelSettings {
            params: ChannelParams::Selcall(SelcallParams { system }),
            ..settings.clone()
        })
        .collect()
}

fn start(
    type_id: &str,
    settings: ChannelSettings,
    rate: f64,
    band: &Band,
) -> Result<Live, ChannelError> {
    let descriptor =
        descriptor_of(type_id).ok_or_else(|| ChannelError::UnknownType(type_id.to_owned()))?;
    let ddc = Ddc::new(rate, descriptor.input_rate_hz, band.center_hz)
        .map_err(|e| ChannelError::InvalidSettings(e.to_string()))?;
    let filter = channel_filter(&settings.params)?;
    let channel = create(
        ChannelCtx {
            input_rate: descriptor.input_rate_hz,
        },
        &settings,
    )?;
    Ok(Live {
        type_id: type_id.to_owned(),
        center_hz: band.center_hz,
        bandwidth_hz: band.bandwidth_hz,
        ddc,
        filter,
        channel,
        seen: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn a_decoder_that_will_not_start_is_explained_and_not_tried_again() {
        let mut confirmer = Confirmer::new();
        let mut candidates = vec![pocsag()];
        let iq = vec![Complex::new(0.0, 0.0); 4_096];
        let unusable_rate = f64::NAN;

        confirmer.confirm(&mut candidates, &iq, unusable_rate, &band(), 148_000_000.0);
        assert!(
            candidates[0].why.contains("will not start"),
            "nothing said why: {:?}",
            candidates[0].why
        );
        assert_eq!(confirmer.refused.len(), 1);

        confirmer.sweep();
        candidates[0].why.clear();
        confirmer.confirm(&mut candidates, &iq, unusable_rate, &band(), 148_000_000.0);
        assert_eq!(confirmer.refused.len(), 1, "the decoder was started again");
        assert!(
            !candidates[0].why.is_empty(),
            "the refusal went unreported on the second window"
        );
    }
}
