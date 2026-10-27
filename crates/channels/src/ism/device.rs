use sdrmm_wire::{IsmCoding, IsmIntegrity, IsmMessage, IsmModulation};

use super::{
    bits::{Bits, hex},
    devices,
    slicer::Timing,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Outcome {
    Skip,
    Mic,
    Decoded,
}

pub(super) type Found = Vec<IsmMessage>;
pub(super) type Decode = fn(&mut Bits, &mut Found) -> Outcome;

impl Outcome {
    pub(super) fn emit(found: &mut Found, message: IsmMessage) -> Self {
        found.push(message);
        Self::Decoded
    }

    pub(super) fn checked(valid: bool, decode: impl FnOnce() -> Self) -> Self {
        if valid { decode() } else { Self::Mic }
    }

    pub(super) fn best(rows: impl Iterator<Item = Self>) -> Self {
        rows.max().unwrap_or(Self::Skip)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Tier {
    First,
    Fallback,
}

pub(super) struct Device {
    pub(super) modulation: IsmModulation,
    pub(super) coding: IsmCoding,
    pub(super) timing: Timing,
    pub(super) integrity: IsmIntegrity,
    pub(super) tier: Tier,
    pub(super) decode: Decode,
}

pub(super) const fn ook(coding: IsmCoding, timing: Timing, decode: Decode) -> Device {
    Device {
        modulation: IsmModulation::Ook,
        coding,
        timing,
        integrity: IsmIntegrity::Checksum,
        tier: Tier::First,
        decode,
    }
}

pub(super) const fn fsk(timing: Timing, decode: Decode) -> Device {
    Device {
        modulation: IsmModulation::Fsk,
        coding: IsmCoding::Pcm,
        ..ook(IsmCoding::Pcm, timing, decode)
    }
}

impl Device {
    pub(super) const fn integrity(self, integrity: IsmIntegrity) -> Self {
        Self { integrity, ..self }
    }

    pub(super) const fn fallback(self) -> Self {
        Self {
            tier: Tier::Fallback,
            ..self
        }
    }
}

pub(super) fn reading(model: &str, id: String, raw: &[u8], bits: usize) -> IsmMessage {
    IsmMessage {
        model: model.to_owned(),
        id,
        raw: hex(raw),
        bits: u16::try_from(bits).unwrap_or(u16::MAX),
        ..IsmMessage::default()
    }
}

pub(super) fn signed(value: u32, width: u32) -> i32 {
    let shift = 32 - width;
    ((value << shift) as i32) >> shift
}

pub(super) fn tenths(value: i32) -> f32 {
    value as f32 * 0.1
}

pub(super) fn fahrenheit(f: f32) -> f32 {
    (f - 32.0) * 5.0 / 9.0
}

pub(super) const DEVICES: &[Device] = devices::ALL;

#[cfg(test)]
pub(super) fn tenths_c(message: &IsmMessage) -> Option<f32> {
    message
        .temperature_c
        .map(|value| (value * 10.0).round() / 10.0)
}
