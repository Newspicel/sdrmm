//! GPS L1 C/A receiver: searches every PRN, tracks up to twelve satellites at once, reads their
//! ephemeris and time, and solves the receiver's position and clock.

use sdrmm_decoder::registry::Native;
use sdrmm_wire::FilterSpec;
use sdrmm_wire::{EventContract, KindSpec, NumberSpec, SettingField};
mod acquire;
mod code;
mod model;
mod nav;
mod solve;
#[cfg(any(test, feature = "synth"))]
pub mod synth;
mod track;
pub use model::*;
use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_wire::{ChannelDescriptor, ChannelSettings, DecoderEvent, DecoderFamily};

use sdrmm_decoder::{
    Channel, ChannelCtx, ChannelError, ChannelOutputs, NativeChannel, check_input_rate,
};
use sdrmm_modem::constellation::{Constellation, tables};

use acquire::{Found, RATE, SAMPLES_PER_MS, Search};
use code::CHIPS;
use nav::Subframe;
use solve::{MAX_SATELLITES, Measurement, SPEED_OF_LIGHT, Solution, solve};
use track::{Step, Tracker};

const HISTORY_MS: usize = 3;
const HISTORY: usize = HISTORY_MS * SAMPLES_PER_MS;
const FIX_EVERY_MS: u64 = 1_000;
/// Weeks before the ten-bit broadcast week number's latest rollover, 7 April 2019.
const WEEK_ROLLOVER: u16 = 2_048;
const HEALTHY: u8 = 0;
const PROMPTS_PER_S: f64 = 1_000.0;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "gnss".to_owned(),
    name: "GNSS lab (GPS L1 C/A)".to_owned(),
    summary: "GPS satellite tracking and position fix".to_owned(),
    family: DecoderFamily::UTILITY,
    verification: sdrmm_wire::Verification::Experimental,
    bandwidth_hz: 2_046_000.0,
    input_rate_hz: RATE,
    has_audio: false,
    emits: vec![GnssFrame::KIND, GnssFix::KIND],
    ..ChannelDescriptor::default()
});

pub struct GnssChannel {
    params: GnssParams,
    pending: Vec<Complex<f32>>,
    history: Vec<Complex<f32>>,
    blocks: u64,
    search: Search,
    trackers: Vec<Tracker>,
    prompts: Vec<Complex<f32>>,
    bpsk: Constellation,
}

fn params(settings: &ChannelSettings) -> Result<GnssParams, ChannelError> {
    sdrmm_decoder::settings_of::<GnssParams>(settings)
}

pub(crate) fn occupied_band() -> (f64, f64) {
    (-1_023_000.0, 1_023_000.0)
}

pub(crate) fn channel_filter() -> FilterSpec {
    FilterSpec::Passthrough
}

impl NativeChannel for GnssChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }
    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        let p = params(&settings)?;
        Ok(Self::build(p))
    }
}

impl Channel for GnssChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let p = params(&settings)?;
        if p != self.params {
            *self = Self::build(p);
        }
        Ok(())
    }

    fn retuned(&mut self) {
        self.pending.clear();
        self.blocks = 0;
        self.search.reset();
        self.trackers.clear();
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.prompts.clear();
        let mut rest = iq;
        while !rest.is_empty() {
            let take = (SAMPLES_PER_MS - self.pending.len()).min(rest.len());
            self.pending.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
            if self.pending.len() == SAMPLES_PER_MS {
                self.process_millisecond(out);
                self.pending.clear();
            }
        }
        self.tap_prompts(out);
    }
}

impl GnssChannel {
    fn build(params: GnssParams) -> Self {
        Self {
            params,
            pending: Vec::with_capacity(SAMPLES_PER_MS),
            history: vec![Complex::default(); HISTORY],
            blocks: 0,
            search: Search::new(params.doppler_hz, params.threshold),
            trackers: Vec::with_capacity(MAX_SATELLITES),
            prompts: Vec::new(),
            bpsk: tables::bpsk(),
        }
    }

    /// The satellite the scope shows: the strongest, so one clean cloud rather than a blur of
    /// every tracked one.
    fn strongest(&self) -> Option<&Tracker> {
        self.trackers
            .iter()
            .max_by(|a, b| a.cn0_db_hz.total_cmp(&b.cn0_db_hz))
    }

    fn tap_prompts(&self, out: &mut ChannelOutputs) {
        if !out.symbols.wanted() || self.prompts.is_empty() {
            return;
        }
        let doppler_hz = self.strongest().map_or(0.0, Tracker::doppler_hz);
        out.symbols
            .linear(&self.prompts, &self.bpsk, PROMPTS_PER_S, doppler_hz);
    }

    /// Absolute sample just past the newest one held.
    fn end(&self) -> u64 {
        self.blocks * SAMPLES_PER_MS as u64
    }

    fn origin(&self) -> u64 {
        self.end().saturating_sub(HISTORY as u64)
    }

    fn process_millisecond(&mut self, out: &mut ChannelOutputs) {
        self.history.copy_within(SAMPLES_PER_MS.., 0);
        self.history[HISTORY - SAMPLES_PER_MS..].copy_from_slice(&self.pending);
        self.blocks += 1;
        if self.blocks < HISTORY_MS as u64 {
            return;
        }
        self.track(out);
        if self.trackers.len() < MAX_SATELLITES {
            self.search(out);
        }
        if self.blocks.is_multiple_of(FIX_EVERY_MS) {
            self.fix(out);
        }
    }

    fn track(&mut self, out: &mut ChannelOutputs) {
        let (origin, end) = (self.origin(), self.end() as f64);
        let shown = out
            .symbols
            .wanted()
            .then(|| self.strongest().map(|tracker| tracker.prn))
            .flatten();
        let mut index = 0;
        while index < self.trackers.len() {
            let mut lost = false;
            while !lost && self.trackers[index].needs_until() <= end {
                let step = self.trackers[index].step(&self.history, origin);
                if shown == Some(self.trackers[index].prn) {
                    self.prompts.push(self.trackers[index].prompt());
                }
                match step {
                    Step::Nothing => {}
                    Step::Subframe(subframe) => {
                        let frame = self.frame(&self.trackers[index], Some(&subframe));
                        out.events.push(DecoderEvent::of(&frame));
                    }
                    Step::Lost => lost = true,
                }
            }
            if lost {
                self.trackers.swap_remove(index);
            } else {
                index += 1;
            }
        }
    }

    fn search(&mut self, out: &mut ChannelOutputs) {
        let start = self.end() - SAMPLES_PER_MS as u64;
        let block = &self.history[HISTORY - SAMPLES_PER_MS..];
        let trackers = &self.trackers;
        let tracked = |prn: u8| trackers.iter().any(|t| t.prn == prn);
        let Some(found) = self.search.feed(block, start, tracked) else {
            return;
        };
        let tracker = Tracker::new(found, self.origin());
        out.events
            .push(DecoderEvent::of(&self.acquired(&tracker, found)));
        self.trackers.push(tracker);
    }

    /// The acquisition's own view: code phase at the newest block's start at the nominal chip
    /// rate, since the search knows no code Doppler.
    fn acquired(&self, tracker: &Tracker, found: Found) -> GnssFrame {
        let block_start = (self.end() - SAMPLES_PER_MS as u64) as f64;
        let chips = (block_start - found.code_start) * CHIPS as f64 / SAMPLES_PER_MS as f64;
        GnssFrame {
            cn0_db_hz: found.cn0_db_hz,
            code_phase_chips: chips.rem_euclid(CHIPS as f64) as f32,
            ..self.frame(tracker, None)
        }
    }

    fn frame(&self, tracker: &Tracker, subframe: Option<&Subframe>) -> GnssFrame {
        let block_start = (self.end() - SAMPLES_PER_MS as u64) as f64;
        GnssFrame {
            prn: tracker.prn,
            doppler_hz: tracker.doppler_hz().round() as f32,
            code_phase_chips: tracker.code_phase_chips(block_start) as f32,
            cn0_db_hz: tracker.cn0_db_hz,
            subframe: subframe.map(|s| s.id),
            tow_seconds: subframe.map(|s| s.next_tow_s),
            week: subframe
                .filter(|s| s.id == 1)
                .map(|s| ((s.data[2] >> 14) & 0x3FF) as u16),
            words: subframe.map_or_else(Vec::new, |s| {
                s.raw.iter().map(|word| format!("{word:08X}")).collect()
            }),
        }
    }

    fn measurements(&self, at: f64) -> ([Measurement; MAX_SATELLITES], usize) {
        let mut measurements = [Measurement::default(); MAX_SATELLITES];
        let mut count = 0;
        for tracker in &self.trackers {
            let (Some(ephemeris), Some(transmit_s)) = (tracker.ephemeris, tracker.transmit_s(at))
            else {
                continue;
            };
            if ephemeris.health != HEALTHY || count == MAX_SATELLITES {
                continue;
            }
            measurements[count] = Measurement {
                prn: tracker.prn,
                transmit_s,
                ephemeris,
            };
            count += 1;
        }
        (measurements, count)
    }

    fn fix(&self, out: &mut ChannelOutputs) {
        let (measurements, count) = self.measurements(self.end() as f64);
        if count < 4 {
            return;
        }
        let Some(solution) = solve(&measurements[..count]) else {
            return;
        };
        out.events
            .push(DecoderEvent::of(&fix_event(&solution, &measurements[0])));
    }
}

fn fix_event(solution: &Solution, any: &Measurement) -> GnssFix {
    let mut satellites = solution.satellites().to_vec();
    satellites.sort_unstable();
    GnssFix {
        lat: solution.at.lat_deg,
        lon: solution.at.lon_deg,
        altitude_m: solution.at.alt_m,
        satellites,
        pdop: solution.pdop as f32,
        hdop: solution.hdop as f32,
        vdop: solution.vdop as f32,
        residual_m: solution.residual_rms_m as f32,
        clock_bias_us: solution.clock_bias_m / SPEED_OF_LIGHT * 1e6,
        week: WEEK_ROLLOVER + any.ephemeris.week,
        tow_seconds: solution.tow_s,
    }
}

pub(crate) fn gnss_decoder() -> Native<GnssParams> {
    let decoder = Native::new::<GnssChannel>(|_| occupied_band())
        .filter(|_| Ok(channel_filter()))
        .home(1_575_420_000.0)
        .form(gnss_form)
        .kinds(gnss_kinds);
    cfg_select! {
        any(test, feature = "synth") => decoder.synth(synth::SIGNALS).recipes(synth::RECIPES),
        _ => decoder,
    }
}

pub(crate) fn gnss_form() -> Vec<SettingField> {
    vec![
        SettingField::new(
            "doppler_hz",
            "Doppler",
            NumberSpec::hz(500.0, 20_000.0, 500.0),
        )
        .title("Symmetric Doppler search span"),
        SettingField::new(
            "threshold",
            "Acquire",
            NumberSpec::new(1.5, 100.0, 0.1).unit("× floor"),
        )
        .title("Correlation peak-to-floor acquisition threshold"),
    ]
}

fn gnss_kinds() -> Vec<KindSpec> {
    vec![gnss_kind(), gnss_fix_kind()]
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod open_sky;
#[cfg(test)]
mod tests;
