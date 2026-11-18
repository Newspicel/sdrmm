//! The APRS encoder: AX.25 UI frames keyed as 1200 baud AFSK or 9600 baud G3RUH on FM.

use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::{HdlcFramer, crc16_x25};
use sdrmm_modem::{
    keying::{Burst, TxQueue},
    packet::{
        AFSK_BAUD, DEVIATION_HZ, G3RUH_BAUD, Keyer, Line,
        ax25::{self, CALL_LEN},
    },
};
use sdrmm_wire::{Control, SettingField};
use serde::{Deserialize, Serialize};

use super::{AprsMode, DESCRIPTOR, MAX_FRAME_BYTES, MIN_FRAME_BYTES, packet_mode};
use sdrmm_decoder::encoder::{
    EncoderBase, EncoderError, Modulator, NativeEncoder, NativeModulator, TxPayload,
};

const TYPE_ID: &str = "aprs";

pub(super) const PREAMBLE_FLAGS: usize = 24;
pub(super) const TRAILING_FLAGS: usize = 2;

pub struct AprsModulator {
    rate: f64,
    pending: TxQueue<bool>,
    staging: Vec<bool>,
    line: Line,
    keyer: Keyer,
    remaining: usize,
    samples_per_symbol: usize,
    level: bool,
    carrier_phase: f64,
    burst: Burst,
}

fn baud(mode: AprsMode) -> f64 {
    match mode {
        AprsMode::Afsk1200 => AFSK_BAUD,
        AprsMode::G3ruh9600 => G3RUH_BAUD,
    }
}

impl AprsModulator {
    #[must_use]
    pub fn ui_frame(source: &str, destination: &str, path: &[&str], info: &str) -> Vec<u8> {
        ax25::ui_frame(source, destination, path, info.as_bytes())
    }

    fn next_level(&mut self) -> Option<bool> {
        let bit = self.pending.pop()?;
        Some(self.line.push(bit))
    }

    fn modulating(&mut self) -> f32 {
        self.keyer.next(self.level, self.rate)
    }
}

/// What an APRS encoder sends: the line its frames are keyed on.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AprsEncoderSettings {
    pub mode: AprsMode,
}

/// The channel a mode is sent on: 1200 baud AFSK fits 12.5 kHz, 9600 baud G3RUH needs 25 kHz.
const fn channel_width_hz(mode: AprsMode) -> f64 {
    match mode {
        AprsMode::Afsk1200 => 12_500.0,
        AprsMode::G3ruh9600 => 25_000.0,
    }
}

impl NativeModulator for AprsModulator {
    type Settings = AprsEncoderSettings;

    fn rate_hz() -> f64 {
        DESCRIPTOR.input_rate_hz
    }

    fn new(settings: &AprsEncoderSettings) -> Result<Self, EncoderError> {
        let rate = DESCRIPTOR.input_rate_hz;
        let mode = settings.mode;
        Ok(Self {
            rate,
            pending: TxQueue::new(TYPE_ID, baud(mode)),
            staging: Vec::new(),
            line: Line::new(packet_mode(mode)),
            keyer: Keyer::new(packet_mode(mode)),
            remaining: 0,
            samples_per_symbol: (rate / baud(mode)) as usize,
            level: false,
            carrier_phase: 0.0,
            burst: Burst::new(rate),
        })
    }
}

#[cfg(test)]
impl AprsModulator {
    /// Keys `bits` as they are, flags and stuffing included, to send what no frame would.
    pub(super) fn key_bits(&mut self, bits: Vec<bool>) -> Result<(), EncoderError> {
        self.pending.accept(bits.len())?;
        self.pending.extend(bits);
        Ok(())
    }
}

impl Modulator for AprsModulator {
    fn submit(&mut self, payload: TxPayload) -> Result<(), EncoderError> {
        let frame = payload.frame(TYPE_ID)?;
        let (min, max) = (MIN_FRAME_BYTES - 2, MAX_FRAME_BYTES - 2);
        if !(min..=max).contains(&frame.len()) {
            return Err(EncoderError::message(
                TYPE_ID,
                format!(
                    "an ax.25 frame is {min}..={max} octets before the fcs, got {}",
                    frame.len()
                ),
            ));
        }
        self.staging.clear();
        if self.pending.is_empty() {
            push_flags(&mut self.staging, PREAMBLE_FLAGS);
        }
        push_frame(&mut self.staging, &frame, 0);
        push_flags(&mut self.staging, TRAILING_FLAGS);
        self.pending.accept(self.staging.len())?;
        self.pending.extend(self.staging.iter().copied());
        Ok(())
    }

    fn generate(&mut self, out: &mut [Complex<f32>]) -> usize {
        let mut written = 0;
        for slot in out {
            if self.remaining == 0
                && let Some(level) = self.next_level()
            {
                self.level = level;
                self.remaining = self.samples_per_symbol;
            }
            let Some(envelope) = self.burst.next(self.remaining > 0) else {
                break;
            };
            let audio = self.modulating();
            self.carrier_phase += TAU * DEVIATION_HZ * f64::from(audio) / self.rate;
            if self.carrier_phase > TAU {
                self.carrier_phase -= TAU;
            } else if self.carrier_phase < -TAU {
                self.carrier_phase += TAU;
            }
            *slot = Complex::from_polar(envelope, self.carrier_phase as f32);
            self.remaining = self.remaining.saturating_sub(1);
            written += 1;
        }
        written
    }
}

pub(super) fn push_flags(bits: &mut Vec<bool>, count: usize) {
    HdlcFramer::new().push_flags(bits, count);
}

/// The frame with its FCS flipped by the bits of `fcs_error`, which only a test sets.
pub(super) fn push_frame(bits: &mut Vec<bool>, frame: &[u8], fcs_error: u16) {
    let mut framer = HdlcFramer::new();
    framer.push_bytes(bits, frame);
    framer.push_bytes(bits, &(crc16_x25(frame) ^ fcs_error).to_le_bytes());
    framer.push_flags(bits, 1);
}

const MAX_PATH_HOPS: usize = 8;

/// One UI frame as the operator writes it.
#[derive(Debug, serde::Deserialize)]
struct AprsMessage {
    source: String,
    #[serde(default = "default_destination")]
    destination: String,
    #[serde(default)]
    path: String,
    text: String,
}

fn default_destination() -> String {
    "APRS".to_owned()
}

fn message_form() -> Vec<SettingField> {
    vec![
        SettingField::new("source", "From", Control::text("N0CALL-9")).title("Your callsign"),
        SettingField::new("destination", "To", Control::text("APRS")).title("Destination address"),
        SettingField::new("path", "Path", Control::text("WIDE1-1,WIDE2-1"))
            .title("Digipeater path, comma separated"),
        SettingField::new("text", "Text", Control::text("!4903.50N/07201.75W-"))
            .title("APRS information field"),
    ]
}

fn checked_call(call: &str, what: &str) -> Result<(), EncoderError> {
    let (base, ssid) = call.split_once('-').unwrap_or((call, "0"));
    let named = !base.is_empty()
        && base.len() <= CALL_LEN
        && base.bytes().all(|byte| byte.is_ascii_alphanumeric());
    if named && ssid.parse::<u8>().is_ok_and(|ssid| ssid <= 15) {
        return Ok(());
    }
    Err(EncoderError::message(
        TYPE_ID,
        format!("{what} `{call}` is not a callsign with an optional -0 to -15"),
    ))
}

pub(super) fn compose(message: &serde_json::Value) -> Result<TxPayload, EncoderError> {
    let message: AprsMessage = serde_json::from_value(message.clone())
        .map_err(|error| EncoderError::message(TYPE_ID, error.to_string()))?;
    let source = message.source.trim();
    let destination = message.destination.trim();
    let hops: Vec<&str> = message
        .path
        .split(',')
        .map(str::trim)
        .filter(|hop| !hop.is_empty())
        .collect();
    checked_call(source, "source")?;
    checked_call(destination, "destination")?;
    if hops.len() > MAX_PATH_HOPS {
        return Err(EncoderError::message(
            TYPE_ID,
            format!(
                "a path holds at most {MAX_PATH_HOPS} hops, got {}",
                hops.len()
            ),
        ));
    }
    for hop in &hops {
        checked_call(hop, "path hop")?;
    }
    if message.text.is_empty() || !message.text.is_ascii() {
        return Err(EncoderError::message(
            TYPE_ID,
            "the text must be ASCII and not empty",
        ));
    }
    Ok(TxPayload::Frame(AprsModulator::ui_frame(
        source,
        destination,
        &hops,
        &message.text,
    )))
}

pub(crate) fn aprs_encoder() -> NativeEncoder<AprsEncoderSettings> {
    NativeEncoder::new::<AprsModulator>(
        EncoderBase {
            type_id: TYPE_ID,
            name: "APRS",
            summary: "APRS packets",
        },
        |s: &AprsEncoderSettings| channel_width_hz(s.mode),
    )
    .form(aprs_encoder_form)
    .messages(message_form, compose)
}

fn aprs_encoder_form() -> Vec<SettingField> {
    vec![SettingField::new("mode", "Mode", Control::choice::<AprsMode>()).title("APRS mode")]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_mode_is_sent_in_its_own_channel_width() {
        assert_eq!(channel_width_hz(AprsMode::Afsk1200), 12_500.0);
        assert_eq!(channel_width_hz(AprsMode::G3ruh9600), 25_000.0);
    }
}
