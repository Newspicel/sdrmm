mod classify;
mod lags;
mod preamble;
mod spectrum;

use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_dsp::{Decimator, FirC, Nco, design_lowpass};
use sdrmm_wire::{
    ChannelDescriptor, ChannelParams, ChannelSettings, DecoderEvent, DecoderFamily,
    MAX_STANAG_WINDOW_S, MIN_STANAG_WINDOW_S, StanagModem, StanagParams, StanagRate, StanagReport,
};

use crate::{ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, check_input_rate};
use classify::Verdict;
use lags::Lags;
use preamble::{Preamble, PreambleSearch};
use spectrum::{Occupancy, Spectrum};

const INPUT_RATE_HZ: f64 = 9_600.0;
const DECIMATION: usize = 2;
pub(crate) const RATE_HZ: f64 = INPUT_RATE_HZ / DECIMATION as f64;
pub(crate) const NOMINAL_CARRIER_HZ: f64 = 1_800.0;
pub(crate) const PASSBAND_LOW_HZ: f64 = 200.0;
pub(crate) const PASSBAND_HIGH_HZ: f64 = 3_600.0;
const PASSBAND_TAPS: usize = 127;
const DECIMATION_TAPS: usize = 31;
const DECIMATION_CUTOFF_HZ: f64 = 2_000.0;
const HOP: usize = RATE_HZ as usize;
const MISSES_TO_LOSE: u8 = 2;
const ALE_CENTER_HZ: f64 = 1_625.0;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "stanag".to_owned(),
    name: "STANAG modem identifier".to_owned(),
    summary: "Names HF military modems".to_owned(),
    family: DecoderFamily::Utility,
    bandwidth_hz: PASSBAND_HIGH_HZ - PASSBAND_LOW_HZ,
    input_rate_hz: INPUT_RATE_HZ,
    has_audio: false,
    decoder_kind: Some("stanag".to_owned()),
    ..ChannelDescriptor::default()
});

fn params(settings: &ChannelSettings) -> Result<StanagParams, ChannelError> {
    let ChannelParams::Stanag(params) = &settings.params else {
        return Err(ChannelError::InvalidSettings(format!(
            "STANAG channel got {} params",
            settings.params.type_id()
        )));
    };
    if !(MIN_STANAG_WINDOW_S..=MAX_STANAG_WINDOW_S).contains(&params.window_s) {
        return Err(ChannelError::InvalidSettings(format!(
            "window must be {MIN_STANAG_WINDOW_S}..={MAX_STANAG_WINDOW_S} s, got {}",
            params.window_s
        )));
    }
    Ok(*params)
}

pub(crate) fn occupied_band() -> (f64, f64) {
    (PASSBAND_LOW_HZ, PASSBAND_HIGH_HZ)
}

pub(crate) fn channel_filter() -> ChannelFilter {
    let half_width = (PASSBAND_HIGH_HZ - PASSBAND_LOW_HZ) / 2.0;
    let center = (PASSBAND_HIGH_HZ + PASSBAND_LOW_HZ) / 2.0;
    ChannelFilter::Sideband(FirC::from_lowpass(
        &design_lowpass(PASSBAND_TAPS, half_width / INPUT_RATE_HZ),
        center / INPUT_RATE_HZ,
    ))
}

fn window_len(params: &StanagParams) -> usize {
    usize::from(params.window_s) * HOP
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Heard {
    modem: StanagModem,
    rate: Option<StanagRate>,
}

struct Analysis {
    lags: Lags,
    spectrum: Spectrum,
    preamble: PreambleSearch,
}

impl Analysis {
    fn new(window: usize) -> Self {
        Self {
            lags: Lags::new(window),
            spectrum: Spectrum::new(),
            preamble: PreambleSearch::new(window),
        }
    }
}

pub struct StanagChannel {
    params: StanagParams,
    frequency_hz: f64,
    mixer: Nco,
    mixed: Vec<Complex<f32>>,
    decimator: Decimator,
    baseband: Vec<Complex<f32>>,
    window: Vec<Complex<f32>>,
    span: usize,
    analysis: Analysis,
    heard: Option<Heard>,
    switching: Option<StanagModem>,
    misses: u8,
    rejected: u32,
}

impl StanagChannel {
    fn forget(&mut self) {
        self.window.clear();
        self.heard = None;
        self.switching = None;
        self.misses = 0;
    }

    fn analyse(&mut self, out: &mut ChannelOutputs) {
        let occupancy = self.analysis.spectrum.measure(&self.window);
        self.analysis.lags.measure(&self.window);
        let comb = self.analysis.spectrum.ale_comb();
        let tracked = self.heard.map(|heard| heard.modem);
        match classify::classify(&mut self.analysis.lags, &comb, tracked) {
            Some(verdict) => self.heard(verdict, occupancy, comb.offset_hz, out),
            None => self.missed(occupancy.occupied, out),
        }
    }

    fn missed(&mut self, occupied: bool, out: &mut ChannelOutputs) {
        self.switching = None;
        if occupied {
            self.rejected = self.rejected.saturating_add(1);
        }
        self.misses = self.misses.saturating_add(1);
        if self.misses >= MISSES_TO_LOSE && self.heard.take().is_some() {
            out.events.push(DecoderEvent::Stanag(StanagReport {
                modem: None,
                confidence: 0.0,
                audio_hz: 0.0,
                frequency_hz: None,
                bandwidth_hz: 0.0,
                rate: None,
            }));
        }
    }

    fn unconfirmed_switch(&mut self, modem: StanagModem) -> bool {
        let switching = self.heard.is_some_and(|heard| heard.modem != modem);
        let unconfirmed = switching && self.switching != Some(modem);
        self.switching = unconfirmed.then_some(modem);
        unconfirmed
    }

    fn heard(
        &mut self,
        verdict: Verdict,
        occupancy: Occupancy,
        comb_offset_hz: f64,
        out: &mut ChannelOutputs,
    ) {
        if self.unconfirmed_switch(verdict.modem) {
            return;
        }
        self.misses = 0;
        let known = self
            .heard
            .filter(|heard| heard.modem == verdict.modem)
            .and_then(|heard| heard.rate);
        let mut offset_hz =
            classify::refine_offset(&mut self.analysis.lags, verdict.modem, occupancy.center_hz);
        let rate = match (verdict.modem, known) {
            (StanagModem::MilStd110a, None) => match self.analysis.preamble.search(&self.window) {
                Preamble::Rate {
                    rate,
                    offset_hz: found,
                } => {
                    offset_hz = found;
                    Some(rate)
                }
                Preamble::Unreadable => {
                    self.rejected = self.rejected.saturating_add(1);
                    None
                }
                Preamble::Missing => None,
            },
            (_, known) => known,
        };
        if verdict.modem == StanagModem::Ale2g {
            offset_hz = ALE_CENTER_HZ - NOMINAL_CARRIER_HZ + comb_offset_hz;
        }
        let now = Heard {
            modem: verdict.modem,
            rate,
        };
        if self.heard == Some(now) {
            return;
        }
        self.heard = Some(now);
        let audio_hz = NOMINAL_CARRIER_HZ + offset_hz;
        out.events.push(DecoderEvent::Stanag(StanagReport {
            modem: Some(verdict.modem),
            confidence: verdict.confidence,
            audio_hz: audio_hz as f32,
            frequency_hz: (self.frequency_hz > 0.0).then_some(self.frequency_hz + audio_hz),
            bandwidth_hz: occupancy.bandwidth_hz as f32,
            rate,
        }));
    }
}

impl ChannelRx for StanagChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        let params = params(&settings)?;
        let window = window_len(&params);
        Ok(Self {
            params,
            frequency_hz: settings.frequency_hz,
            mixer: Nco::new(-NOMINAL_CARRIER_HZ as f32, INPUT_RATE_HZ as f32),
            mixed: Vec::new(),
            decimator: Decimator::new(
                &design_lowpass(DECIMATION_TAPS, DECIMATION_CUTOFF_HZ / INPUT_RATE_HZ),
                DECIMATION,
            ),
            baseband: Vec::new(),
            window: Vec::with_capacity(window),
            span: window,
            analysis: Analysis::new(window),
            heard: None,
            switching: None,
            misses: 0,
            rejected: 0,
        })
    }

    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let params = params(&settings)?;
        if params != self.params {
            let window = window_len(&params);
            self.window = Vec::with_capacity(window);
            self.span = window;
            self.analysis = Analysis::new(window);
        }
        if params != self.params || settings.frequency_hz != self.frequency_hz {
            self.forget();
        }
        self.params = params;
        self.frequency_hz = settings.frequency_hz;
        Ok(())
    }

    fn retuned(&mut self) {
        self.forget();
    }

    fn needs_gated_input(&self) -> bool {
        false
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.mixed.resize(iq.len(), Complex::default());
        self.mixer.mix_into(iq, &mut self.mixed);
        self.decimator.process(&self.mixed, &mut self.baseband);
        let mut taken = 0;
        while taken < self.baseband.len() {
            let room = self.span - self.window.len();
            let end = (taken + room).min(self.baseband.len());
            self.window.extend_from_slice(&self.baseband[taken..end]);
            taken = end;
            if self.window.len() == self.span {
                self.analyse(out);
                self.window.drain(..HOP);
            }
        }
    }

    fn rejected(&self) -> u32 {
        self.rejected
    }
}

#[cfg(test)]
mod tests;
