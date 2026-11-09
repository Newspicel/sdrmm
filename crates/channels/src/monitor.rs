mod decoder;
mod detect;

use std::collections::VecDeque;

use decoder::Decoder;
use num_complex::Complex;
use sdrmm_wire::{
    DecoderEvent, IdentSignal, Modulation, SpectrumMonitorNode, Transmission, TransmissionState,
};

use crate::{ChannelError, ident};

const MAX_TRACKS: usize = 32;
const MAX_HISTORY_SAMPLES: usize = 8 * 1024 * 1024;
const HANG_SECONDS: f64 = 0.3;
/// How long an audio clip runs before the transmission is cut and the clip handed over. A track
/// without audio is not cut: its decoded events are the live view.
const SEGMENT_SECONDS: f64 = 30.0;
const IDENTIFY_SECONDS: f64 = 0.6;
/// How often an unconfirmed track is identified again, at first and at most. The interval
/// doubles while the answer stays the same: each look reads `IDENTIFY_SECONDS` of IQ.
const REFRESH_SECONDS: f64 = 0.2;
const MAX_REFRESH_SECONDS: f64 = 3.2;
const REJECT_SECONDS: f64 = 1.0;
/// How many times a spot's hold doubles while it keeps failing identification: a weak carrier
/// or spur that never identifies is retried every 32 s, not every second.
const MAX_STRIKES: u32 = 5;
/// How long a spot that stopped failing keeps its strikes.
const FORGET_SECONDS: f64 = 60.0;
const RETRY_GAIN_DB: f32 = 6.0;
const MAX_REJECTED: usize = 512;
/// How far, as a share of its bandwidth, a signal drifts before its decoders retune. Smaller
/// steps are the estimate's own jitter, which breaks an OFDM decoder's frequency lock.
const RETUNE_FRACTION: f64 = 0.05;

pub struct MonitorOutput {
    pub transmission: u64,
    pub frequency_hz: f64,
    pub event: DecoderEvent,
    pub audio: Vec<i16>,
}

struct Track {
    id: u64,
    signal: IdentSignal,
    segment: u64,
    last_seen: u64,
    decoders: Vec<Decoder>,
    choices: VecDeque<String>,
    trial_at: u64,
    identified_at: u64,
    refresh_seconds: f64,
    band: ident::detect::Band,
    tuned_hz: f64,
    tried: Vec<String>,
    error: Option<String>,
}

struct Rejected {
    frequency_hz: f64,
    bandwidth_hz: f64,
    snr_db: f32,
    until: u64,
    strikes: u32,
}

fn same_signal(frequency_hz: f64, bandwidth_hz: f64, band_hz: f64, band_width_hz: f64) -> bool {
    (frequency_hz - band_hz).abs() < ((bandwidth_hz + band_width_hz) * 0.4).max(1500.0)
}

pub struct SpectrumMonitor {
    settings: SpectrumMonitorNode,
    rate: f64,
    center: f64,
    next: Option<u64>,
    next_id: u64,
    window: Vec<Complex<f32>>,
    history: VecDeque<Complex<f32>>,
    history_capacity: usize,
    detector: detect::Detector,
    tracks: Vec<Track>,
    rejected: Vec<Rejected>,
    overloaded: bool,
    clipping: bool,
    catalog: ident::Catalog,
    meter: ident::Meter,
}

impl SpectrumMonitor {
    pub fn new(
        rate: f64,
        center: f64,
        settings: SpectrumMonitorNode,
    ) -> Result<Self, ChannelError> {
        if !settings.valid() {
            return Err(ChannelError::InvalidSettings(
                "monitor needs confidence between 0 and 1 and start below stop".to_owned(),
            ));
        }
        if !rate.is_finite() || !(8000.0..=64_000_000.0).contains(&rate) || !center.is_finite() {
            return Err(ChannelError::InvalidSettings(
                "monitor requires 8 kHz to 64 MHz IQ and a finite center".to_owned(),
            ));
        }
        let history_capacity = ((rate * 2.0) as usize).clamp(4096, MAX_HISTORY_SAMPLES);
        Ok(Self {
            settings,
            rate,
            center,
            next: None,
            next_id: 1,
            window: Vec::with_capacity((rate * 0.1) as usize),
            history: VecDeque::with_capacity(history_capacity),
            history_capacity,
            detector: detect::Detector::new(rate),
            tracks: Vec::new(),
            rejected: Vec::with_capacity(MAX_REJECTED),
            overloaded: false,
            clipping: false,
            catalog: ident::Catalog::registered(),
            meter: ident::Meter::new(),
        })
    }

    pub fn process(&mut self, iq: &[Complex<f32>], start: u64) -> Vec<MonitorOutput> {
        let mut output = Vec::new();
        if let Some(next) = self.next.filter(|&next| next != start) {
            output.extend(self.finish(TransmissionState::Interrupted, Some("IQ samples lost")));
            let lost = if start > next {
                format!("{} IQ samples lost", start - next)
            } else {
                "IQ samples out of order".to_owned()
            };
            output.push(self.problem(&lost, start));
            self.history.clear();
            self.window.clear();
        }
        let window_len = ((self.rate * 0.1) as usize).max(4096);
        let mut position = start;
        let mut remaining = iq;
        while !remaining.is_empty() {
            let take = (window_len - self.window.len()).min(remaining.len());
            let samples = &remaining[..take];
            for track in &mut self.tracks {
                feed(track, samples, &mut output, &self.catalog);
            }
            self.window.extend_from_slice(samples);
            let excess = (self.history.len() + samples.len()).saturating_sub(self.history_capacity);
            self.history.drain(..excess.min(self.history.len()));
            self.history.extend(samples.iter().copied());
            position += take as u64;
            if self.window.len() == window_len {
                self.survey(position, &mut output);
                self.window.clear();
            }
            remaining = &remaining[take..];
        }
        self.next = Some(position);
        output
    }

    fn survey(&mut self, end: u64, output: &mut Vec<MonitorOutput>) {
        let bands = self.detector.measure(&self.window);
        let clipping = sdrmm_dsp::clip::clips(&self.window);
        let forget = (self.rate * FORGET_SECONDS) as u64;
        self.rejected
            .retain(|rejected| rejected.until.saturating_add(forget) > end);
        let mut overflow = false;
        let clear = bands.clear.into_iter().map(|band| (band, false));
        for (band, faint) in clear.chain(bands.faint.into_iter().map(|band| (band, true))) {
            let frequency = self.center + band.center_hz;
            if !self.settings.covers(frequency) {
                continue;
            }
            if let Some(track) = self.tracks.iter_mut().find(|track| {
                same_signal(
                    track.signal.frequency_hz,
                    track.signal.bandwidth_hz,
                    frequency,
                    band.bandwidth_hz,
                )
            }) {
                track.last_seen = end;
                track.signal.snr_db = band.snr_db;
                if band.bandwidth_hz >= track.band.bandwidth_hz * 0.75 {
                    let offset = track.band.center_hz * 0.9 + band.center_hz * 0.1;
                    if (offset - track.tuned_hz).abs() > track.band.bandwidth_hz * RETUNE_FRACTION {
                        for decoder in &mut track.decoders {
                            decoder.retune(offset);
                        }
                        track.tuned_hz = offset;
                    }
                    track.band = ident::detect::Band {
                        center_hz: offset,
                        bandwidth_hz: track.band.bandwidth_hz.max(band.bandwidth_hz),
                        ..band
                    };
                    track.signal.center_offset_hz = offset;
                    track.signal.frequency_hz = self.center + offset;
                }
                continue;
            }
            if clipping || self.recently_rejected(frequency, &band, end) {
                continue;
            }
            let signal = if faint {
                self.identify_retained(&band)
            } else {
                ident::identify(
                    &self.window,
                    self.rate,
                    &band,
                    self.center,
                    &self.catalog,
                    &mut self.meter,
                )
            };
            if signal.confidence < self.settings.min_confidence
                || !self.wanted(&signal)
                || faint && signal.modulation != Modulation::Chirp
            {
                self.reject(frequency, &band, end);
                continue;
            }
            if self.tracks.len() >= MAX_TRACKS {
                overflow = true;
                continue;
            }
            let start = end.saturating_sub(self.history.len() as u64);
            let mut track = Track {
                id: self.next_id,
                choices: decoder::choices(&signal, &self.settings, &self.catalog).into(),
                signal,
                segment: start,
                last_seen: end,
                decoders: Vec::new(),
                trial_at: end,
                identified_at: end,
                refresh_seconds: REFRESH_SECONDS,
                tuned_hz: band.center_hz,
                band,
                tried: Vec::new(),
                error: None,
            };
            self.next_id += 1;
            self.start_trials(&mut track);
            let buffered = self.history.make_contiguous();
            feed(&mut track, buffered, output, &self.catalog);
            self.tracks.push(track);
        }
        if clipping && !self.clipping {
            output.push(self.problem(
                "front end clipping; new signals not identified until it stops",
                end,
            ));
        }
        self.clipping = clipping;
        if overflow && !self.overloaded {
            output.push(self.problem(
                "32 simultaneous signals reached; additional signals skipped",
                end,
            ));
        }
        self.overloaded = overflow;
        let mut tracks = std::mem::take(&mut self.tracks);
        for mut track in tracks.drain(..) {
            if end.saturating_sub(track.last_seen) as f64 >= self.rate * HANG_SECONDS
                || !self.refresh(&mut track, end, output)
            {
                output.push(transmission(
                    &mut track,
                    end,
                    self.rate,
                    TransmissionState::Completed,
                    &self.catalog,
                ));
                continue;
            }
            if end.saturating_sub(track.segment) as f64 >= self.rate * SEGMENT_SECONDS
                && track
                    .decoders
                    .iter()
                    .any(|decoder| !decoder.audio.is_empty())
            {
                output.push(transmission(
                    &mut track,
                    end,
                    self.rate,
                    TransmissionState::Continued,
                    &self.catalog,
                ));
                track.segment = end;
            }
            self.tracks.push(track);
        }
    }

    fn refresh(&mut self, track: &mut Track, end: u64, output: &mut Vec<MonitorOutput>) -> bool {
        if track.decoders.iter().any(|decoder| decoder.verified) {
            return true;
        }
        if end == track.last_seen
            && end.saturating_sub(track.identified_at) as f64 >= self.rate * track.refresh_seconds
        {
            track.identified_at = end;
            let signal = self.identify_retained(&track.band);
            if signal.confidence < self.settings.min_confidence {
                return false;
            }
            let confirmed = |kind: &str| {
                signal.candidates.iter().any(|candidate| {
                    candidate.confirmed && candidate.type_id.as_deref() == Some(kind)
                })
            };
            if signal
                .candidates
                .iter()
                .any(|candidate| candidate.confirmed)
            {
                track.decoders.retain(|decoder| confirmed(&decoder.kind));
                track.choices.clear();
                track.tried.retain(|kind| !confirmed(kind));
            }
            for kind in decoder::choices(&signal, &self.settings, &self.catalog) {
                if !track.tried.contains(&kind)
                    && !track.choices.contains(&kind)
                    && !track.decoders.iter().any(|decoder| decoder.kind == kind)
                {
                    track.choices.push_back(kind);
                }
            }
            track.refresh_seconds = if same_identity(&signal, &track.signal) {
                (track.refresh_seconds * 2.0).min(MAX_REFRESH_SECONDS)
            } else {
                REFRESH_SECONDS
            };
            track.signal = signal;
        }
        if end.saturating_sub(track.trial_at) as f64 >= self.rate * 2.0 && !track.choices.is_empty()
        {
            track
                .decoders
                .retain(|decoder| decoder.selected(&track.signal, &self.catalog));
            track.trial_at = end;
        }
        let first = track.decoders.len();
        self.start_trials(track);
        if track.decoders.len() > first
            && end.saturating_sub(self.history.len() as u64) > track.segment
        {
            track.error = Some("decoder retry starts after retained IQ".to_owned());
        }
        for decoder in &mut track.decoders[first..] {
            let (events, error) = decoder.process(self.history.make_contiguous(), &self.catalog);
            if error.is_some() {
                track.error = error;
            }
            for event in events {
                output.push(MonitorOutput {
                    transmission: track.id,
                    frequency_hz: track.signal.frequency_hz,
                    event,
                    audio: Vec::new(),
                });
            }
        }
        true
    }

    fn identify_retained(&mut self, band: &ident::detect::Band) -> IdentSignal {
        let (rate, center) = (self.rate, self.center);
        let history = self.history.make_contiguous();
        let start = history
            .len()
            .saturating_sub((rate * IDENTIFY_SECONDS) as usize);
        ident::identify(
            &history[start..],
            rate,
            band,
            center,
            &self.catalog,
            &mut self.meter,
        )
    }

    fn recently_rejected(&self, frequency_hz: f64, band: &ident::detect::Band, end: u64) -> bool {
        self.rejected.iter().any(|rejected| {
            rejected.until > end
                && band.snr_db < rejected.snr_db + RETRY_GAIN_DB
                && same_signal(
                    rejected.frequency_hz,
                    rejected.bandwidth_hz,
                    frequency_hz,
                    band.bandwidth_hz,
                )
        })
    }

    fn reject(&mut self, frequency_hz: f64, band: &ident::detect::Band, end: u64) {
        let known = self.rejected.iter().position(|rejected| {
            same_signal(
                rejected.frequency_hz,
                rejected.bandwidth_hz,
                frequency_hz,
                band.bandwidth_hz,
            )
        });
        let strikes = known.map_or(0, |index| {
            (self.rejected.swap_remove(index).strikes + 1).min(MAX_STRIKES)
        });
        if self.rejected.len() == MAX_REJECTED
            && let Some(oldest) = (0..self.rejected.len()).min_by_key(|&i| self.rejected[i].until)
        {
            self.rejected.swap_remove(oldest);
        }
        let hold = self.rate * REJECT_SECONDS * f64::from(1u32 << strikes);
        self.rejected.push(Rejected {
            frequency_hz,
            bandwidth_hz: band.bandwidth_hz,
            snr_db: band.snr_db,
            until: end + hold as u64,
            strikes,
        });
    }

    fn wanted(&self, signal: &IdentSignal) -> bool {
        signal
            .candidates
            .iter()
            .find_map(|candidate| candidate.type_id.as_deref())
            .map_or(self.settings.report_unidentified, |kind| {
                self.settings.decodes(kind)
            })
    }

    fn start_trials(&self, track: &mut Track) {
        for _ in track.decoders.len()..3 {
            let Some(kind) = track.choices.pop_front() else {
                break;
            };
            track.tried.push(kind.clone());
            match Decoder::new(
                &kind,
                self.rate,
                &track.signal,
                self.settings.keep_clips,
                &self.catalog,
            ) {
                Ok(decoder) => track.decoders.push(decoder),
                Err(error) => track.error = Some(error.to_string()),
            }
        }
    }

    pub fn finish(&mut self, state: TransmissionState, error: Option<&str>) -> Vec<MonitorOutput> {
        let end = self.next.unwrap_or(0);
        let mut output = Vec::new();
        if !self.window.is_empty() {
            self.survey(end, &mut output);
            self.window.clear();
        }
        output.extend(self.tracks.drain(..).map(|mut track| {
            if let Some(error) = error {
                track.error = Some(error.to_owned());
            }
            transmission(&mut track, end, self.rate, state, &self.catalog)
        }));
        output
    }

    fn problem(&self, error: &str, at: u64) -> MonitorOutput {
        MonitorOutput {
            transmission: 0,
            frequency_hz: self.center,
            audio: Vec::new(),
            event: DecoderEvent::of(&Transmission {
                id: 0,
                state: TransmissionState::Problem,
                signal: IdentSignal::default(),
                start_sample: at,
                end_sample: at,
                sample_rate_hz: self.rate,
                duration_ms: 0,
                started_at: None,
                ended_at: None,
                decoder: None,
                decoder_confirmed: false,
                audio: None,
                error: Some(error.to_owned()),
            }),
        }
    }
}

fn same_identity(signal: &IdentSignal, previous: &IdentSignal) -> bool {
    let best = |signal: &IdentSignal| {
        signal
            .candidates
            .first()
            .map(|candidate| (candidate.type_id.clone(), candidate.confirmed))
    };
    signal.modulation == previous.modulation && best(signal) == best(previous)
}

fn feed(
    track: &mut Track,
    iq: &[Complex<f32>],
    output: &mut Vec<MonitorOutput>,
    catalog: &ident::Catalog,
) {
    for decoder in &mut track.decoders {
        let (events, error) = decoder.process(iq, catalog);
        if error.is_some() {
            track.error = error;
        }
        for event in events {
            output.push(MonitorOutput {
                transmission: track.id,
                frequency_hz: track.signal.frequency_hz,
                event,
                audio: Vec::new(),
            });
        }
    }
    if let Some(chosen) = track.decoders.iter().position(|decoder| decoder.verified) {
        let decoder = track.decoders.remove(chosen);
        track.decoders.clear();
        track.decoders.push(decoder);
        track.choices.clear();
    }
}

fn transmission(
    track: &mut Track,
    end: u64,
    rate: f64,
    state: TransmissionState,
    catalog: &ident::Catalog,
) -> MonitorOutput {
    let selected = track
        .decoders
        .iter_mut()
        .filter(|decoder| decoder.selected(&track.signal, catalog))
        .max_by_key(|decoder| decoder.verified);
    let decoder_confirmed = selected.as_ref().is_some_and(|decoder| decoder.verified);
    let (decoder, audio) = selected.map_or((None, Vec::new()), |decoder| {
        (
            Some(decoder.kind.clone()),
            std::mem::take(&mut decoder.audio),
        )
    });
    for decoder in &mut track.decoders {
        decoder.audio.clear();
    }
    MonitorOutput {
        transmission: track.id,
        frequency_hz: track.signal.frequency_hz,
        audio,
        event: DecoderEvent::of(&Transmission {
            id: track.id,
            state,
            signal: track.signal.clone(),
            start_sample: track.segment,
            end_sample: end,
            sample_rate_hz: rate,
            duration_ms: ((end.saturating_sub(track.segment)) as f64 * 1000.0 / rate) as u64,
            started_at: None,
            ended_at: None,
            decoder,
            decoder_confirmed,
            audio: None,
            error: track.error.clone(),
        }),
    }
}

#[cfg(test)]
mod tests;
