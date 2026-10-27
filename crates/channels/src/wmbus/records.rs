use sdrmm_wire::{WmbusFunction, WmbusQuantity, WmbusRecord};

use crate::datalink::hex;

const FILLER: u8 = 0x2F;
const MANUFACTURER: u8 = 0x0F;
const MORE_RECORDS: u8 = 0x1F;
const EXTENSION: u8 = 0x80;
const MAX_DIFE: usize = 10;
const MAX_VIFE: usize = 10;
const PLAIN_TEXT_VIF: u8 = 0x7C;
const TABLE_FB: u8 = 0x7B;
const TABLE_FD: u8 = 0x7D;
const SECONDS: [f64; 4] = [1.0, 60.0, 3_600.0, 86_400.0];
const J_PER_WH: f64 = 3_600.0;
const LVAR_POSITIVE_BCD: u8 = 0xC0;
const LVAR_NEGATIVE_BCD: u8 = 0xD0;
const LVAR_BINARY: u8 = 0xE0;
const LVAR_RESERVED: u8 = 0xF0;
const LVAR_MAX_NUMBER_BYTES: usize = 8;

#[derive(Clone, Copy)]
enum Data {
    None,
    Integer(usize),
    Real,
    Bcd(usize),
    NegativeBcd(usize),
    Variable,
    Selection,
}

#[derive(Clone, Copy)]
struct Scale {
    quantity: WmbusQuantity,
    exponent: i32,
    factor: f64,
}

impl Scale {
    const fn of(quantity: WmbusQuantity, exponent: i32) -> Self {
        Self {
            quantity,
            exponent,
            factor: 1.0,
        }
    }

    const fn times(self, factor: f64) -> Self {
        Self { factor, ..self }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn next(&mut self) -> Option<u8> {
        let byte = *self.bytes.get(self.at)?;
        self.at += 1;
        Some(byte)
    }

    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let slice = self.bytes.get(self.at..self.at + len)?;
        self.at += len;
        Some(slice)
    }
}

pub(super) fn parse(bytes: &[u8]) -> Vec<WmbusRecord> {
    let mut reader = Reader { bytes, at: 0 };
    let mut records = Vec::new();
    while let Some(dif) = reader.next() {
        match dif {
            FILLER => continue,
            MANUFACTURER | MORE_RECORDS => break,
            _ => {}
        }
        match record(dif, &mut reader) {
            Some(Some(found)) => records.push(found),
            Some(None) => {}
            None => break,
        }
    }
    records
}

fn record(dif: u8, reader: &mut Reader<'_>) -> Option<Option<WmbusRecord>> {
    let function = match dif >> 4 & 0x03 {
        0 => WmbusFunction::Instantaneous,
        1 => WmbusFunction::Maximum,
        2 => WmbusFunction::Minimum,
        _ => WmbusFunction::Error,
    };
    let mut storage = u32::from(dif >> 6 & 1);
    let mut tariff = 0u32;
    let mut extended = dif & EXTENSION != 0;
    for depth in 0..MAX_DIFE as u32 {
        if !extended {
            break;
        }
        let dife = reader.next()?;
        storage |= u32::from(dife & 0x0F)
            .checked_shl(1 + 4 * depth)
            .unwrap_or(0);
        tariff |= u32::from(dife >> 4 & 0x03)
            .checked_shl(2 * depth)
            .unwrap_or(0);
        extended = dife & EXTENSION != 0;
    }
    let data = data_kind(dif & 0x0F);
    let vif = reader.next()?;
    let scale = vif_scale(vif, reader)?;
    let (data, raw) = match data {
        Data::None => return Some(None),
        Data::Integer(len) | Data::Bcd(len) | Data::NegativeBcd(len) => (data, reader.take(len)?),
        Data::Real => (data, reader.take(4)?),
        Data::Variable => variable(reader)?,
        Data::Selection => return Some(None),
    };
    let (value, text) = interpret(data, raw, scale);
    Some(Some(WmbusRecord {
        quantity: scale.quantity,
        function,
        storage,
        tariff,
        value,
        text,
    }))
}

fn variable<'a>(reader: &mut Reader<'a>) -> Option<(Data, &'a [u8])> {
    let lvar = reader.next()?;
    let number = |first: u8| {
        let len = usize::from(lvar - first);
        (len <= LVAR_MAX_NUMBER_BYTES).then_some(len)
    };
    let (data, len) = match lvar {
        0..LVAR_POSITIVE_BCD => (Data::Variable, usize::from(lvar)),
        LVAR_POSITIVE_BCD..LVAR_NEGATIVE_BCD => {
            let len = number(LVAR_POSITIVE_BCD)?;
            (Data::Bcd(len), len)
        }
        LVAR_NEGATIVE_BCD..LVAR_BINARY => {
            let len = number(LVAR_NEGATIVE_BCD)?;
            (Data::NegativeBcd(len), len)
        }
        LVAR_BINARY..LVAR_RESERVED => {
            let len = number(LVAR_BINARY)?;
            (Data::Integer(len), len)
        }
        _ => return None,
    };
    Some((data, reader.take(len)?))
}

fn data_kind(code: u8) -> Data {
    match code {
        0x1 => Data::Integer(1),
        0x2 => Data::Integer(2),
        0x3 => Data::Integer(3),
        0x4 => Data::Integer(4),
        0x5 => Data::Real,
        0x6 => Data::Integer(6),
        0x7 => Data::Integer(8),
        0x8 => Data::Selection,
        0x9 => Data::Bcd(1),
        0xA => Data::Bcd(2),
        0xB => Data::Bcd(3),
        0xC => Data::Bcd(4),
        0xD => Data::Variable,
        0xE => Data::Bcd(6),
        _ => Data::None,
    }
}

fn vif_scale(vif: u8, reader: &mut Reader<'_>) -> Option<Scale> {
    let mut scale = match vif & 0x7F {
        TABLE_FB => fb_scale(reader.next()?),
        TABLE_FD => fd_scale(reader.next()?),
        PLAIN_TEXT_VIF => {
            let len = usize::from(reader.next()?);
            reader.take(len)?;
            Scale::of(WmbusQuantity::Other, 0)
        }
        code => primary_scale(code),
    };
    let mut extended = vif & EXTENSION != 0;
    if matches!(vif & 0x7F, TABLE_FB | TABLE_FD) {
        extended = reader.bytes.get(reader.at - 1)? & EXTENSION != 0;
    }
    for _ in 0..MAX_VIFE {
        if !extended {
            break;
        }
        let vife = reader.next()?;
        scale = correction(scale, vife & 0x7F);
        extended = vife & EXTENSION != 0;
    }
    Some(scale)
}

fn correction(scale: Scale, vife: u8) -> Scale {
    match vife {
        0x70..=0x77 => Scale {
            exponent: scale.exponent + i32::from(vife & 0x07) - 6,
            ..scale
        },
        0x7D => Scale {
            exponent: scale.exponent + 3,
            ..scale
        },
        _ => scale,
    }
}

fn primary_scale(code: u8) -> Scale {
    let n = i32::from(code & 0x07);
    let nn = i32::from(code & 0x03);
    let time = SECONDS[usize::from(code & 0x03)];
    match code {
        0x00..=0x07 => Scale::of(WmbusQuantity::Energy, n - 3),
        0x08..=0x0F => Scale::of(WmbusQuantity::Energy, n).times(1.0 / J_PER_WH),
        0x10..=0x17 => Scale::of(WmbusQuantity::Volume, n - 6),
        0x18..=0x1F => Scale::of(WmbusQuantity::Mass, n - 3),
        0x20..=0x23 => Scale::of(WmbusQuantity::OnTime, 0).times(time),
        0x24..=0x27 => Scale::of(WmbusQuantity::OperatingTime, 0).times(time),
        0x28..=0x2F => Scale::of(WmbusQuantity::Power, n - 3),
        0x30..=0x37 => Scale::of(WmbusQuantity::Power, n).times(1.0 / J_PER_WH),
        0x38..=0x3F => Scale::of(WmbusQuantity::VolumeFlow, n - 6),
        0x40..=0x47 => Scale::of(WmbusQuantity::VolumeFlow, n - 7).times(60.0),
        0x48..=0x4F => Scale::of(WmbusQuantity::VolumeFlow, n - 9).times(3_600.0),
        0x50..=0x57 => Scale::of(WmbusQuantity::MassFlow, n - 3),
        0x58..=0x5B => Scale::of(WmbusQuantity::FlowTemperature, nn - 3),
        0x5C..=0x5F => Scale::of(WmbusQuantity::ReturnTemperature, nn - 3),
        0x60..=0x63 => Scale::of(WmbusQuantity::TemperatureDifference, nn - 3),
        0x64..=0x67 => Scale::of(WmbusQuantity::ExternalTemperature, nn - 3),
        0x68..=0x6B => Scale::of(WmbusQuantity::Pressure, nn - 3),
        0x6C => Scale::of(WmbusQuantity::Date, 0),
        0x6D => Scale::of(WmbusQuantity::DateTime, 0),
        0x6E => Scale::of(WmbusQuantity::HeatCostUnits, 0),
        0x70..=0x73 => Scale::of(WmbusQuantity::AveragingDuration, 0).times(time),
        0x74..=0x77 => Scale::of(WmbusQuantity::ActualityDuration, 0).times(time),
        0x78 => Scale::of(WmbusQuantity::FabricationNumber, 0),
        0x79 => Scale::of(WmbusQuantity::Identification, 0),
        _ => Scale::of(WmbusQuantity::Other, 0),
    }
}

fn fb_scale(code: u8) -> Scale {
    let n = i32::from(code & 0x01);
    match code & 0x7F {
        0x00 | 0x01 => Scale::of(WmbusQuantity::Energy, n + 5),
        0x08 | 0x09 => Scale::of(WmbusQuantity::Energy, n + 8).times(1.0 / J_PER_WH),
        0x10 | 0x11 => Scale::of(WmbusQuantity::Volume, n + 2),
        0x1A | 0x1B => Scale::of(WmbusQuantity::Humidity, n - 1),
        0x28 | 0x29 => Scale::of(WmbusQuantity::Power, n + 5),
        _ => Scale::of(WmbusQuantity::Other, 0),
    }
}

fn fd_scale(code: u8) -> Scale {
    match code & 0x7F {
        0x17 => Scale::of(WmbusQuantity::ErrorFlags, 0),
        0x3A => Scale::of(WmbusQuantity::Dimensionless, 0),
        0x74 => Scale::of(WmbusQuantity::BatteryDays, 0),
        _ => Scale::of(WmbusQuantity::Other, 0),
    }
}

fn interpret(data: Data, raw: &[u8], scale: Scale) -> (Option<f64>, Option<String>) {
    match scale.quantity {
        WmbusQuantity::Date if raw.len() == 2 => return (None, date(raw)),
        WmbusQuantity::DateTime if raw.len() == 4 => return (None, date_time(raw)),
        WmbusQuantity::ErrorFlags | WmbusQuantity::Other => {
            return (None, Some(hex(raw)));
        }
        _ => {}
    }
    let number = match data {
        Data::Integer(_) => Some(signed(raw)),
        Data::Bcd(_) => bcd(raw),
        Data::NegativeBcd(_) => bcd(raw).map(|value| -value),
        Data::Real => raw
            .try_into()
            .ok()
            .map(|bytes: [u8; 4]| f64::from(f32::from_le_bytes(bytes))),
        Data::Variable => return (None, Some(text(raw))),
        Data::None | Data::Selection => None,
    };
    match number {
        Some(value) => (Some(scaled(value, scale)), None),
        None => (None, Some(hex(raw))),
    }
}

fn scaled(value: f64, scale: Scale) -> f64 {
    let exact = value * 10f64.powi(scale.exponent) * scale.factor;
    let digits = (-scale.exponent).clamp(0, 9) + if scale.factor == 1.0 { 0 } else { 4 };
    let rounded = format!("{exact:.*}", digits as usize);
    rounded.parse().unwrap_or(exact)
}

fn signed(raw: &[u8]) -> f64 {
    if raw.is_empty() {
        return 0.0;
    }
    let mut bytes = [0u8; 8];
    bytes[..raw.len()].copy_from_slice(raw);
    let shift = 64 - 8 * raw.len() as u32;
    ((i64::from_le_bytes(bytes) << shift) >> shift) as f64
}

fn bcd(raw: &[u8]) -> Option<f64> {
    let mut value = 0i64;
    let mut negative = false;
    for (index, &byte) in raw.iter().enumerate().rev() {
        let (high, low) = (byte >> 4, byte & 0x0F);
        let high = if index == raw.len() - 1 && high == 0x0F {
            negative = true;
            0
        } else {
            high
        };
        if high > 9 || low > 9 {
            return None;
        }
        value = value * 100 + i64::from(high) * 10 + i64::from(low);
    }
    Some(if negative { -value } else { value } as f64)
}

fn text(raw: &[u8]) -> String {
    raw.iter().rev().map(|&byte| char::from(byte)).collect()
}

fn date(raw: &[u8]) -> Option<String> {
    let (low, high) = (raw[0], raw[1]);
    let day = low & 0x1F;
    let month = high & 0x0F;
    let year = (low & 0xE0) >> 5 | (high & 0xF0) >> 1;
    ((1..=31).contains(&day) && (1..=12).contains(&month))
        .then(|| format!("{:04}-{month:02}-{day:02}", 2000 + u32::from(year)))
}

fn date_time(raw: &[u8]) -> Option<String> {
    let minute = raw[0] & 0x3F;
    let hour = raw[1] & 0x1F;
    let day_part = date(&raw[2..])?;
    (minute < 60 && hour < 24).then(|| format!("{day_part} {hour:02}:{minute:02}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(records: &[WmbusRecord], quantity: WmbusQuantity, storage: u32) -> Option<f64> {
        records
            .iter()
            .find(|record| record.quantity == quantity && record.storage == storage)
            .and_then(|record| record.value)
    }

    #[test]
    fn a_water_meter_reports_volume_flow_and_history() {
        let bytes = [
            0x2F, 0x2F, 0x04, 0x13, 0x89, 0xE2, 0x01, 0x00, 0x02, 0x3B, 0x00, 0x00, 0x44, 0x13,
            0x40, 0xD3, 0x01, 0x00, 0x42, 0x6C, 0xBF, 0x2C, 0x04, 0x6D, 0x1E, 0x0D, 0x7F, 0x2C,
            0x02, 0xFD, 0x17, 0x00, 0x00, 0x2F, 0x2F,
        ];
        let records = parse(&bytes);
        assert_eq!(reading(&records, WmbusQuantity::Volume, 0), Some(123.529));
        assert_eq!(reading(&records, WmbusQuantity::VolumeFlow, 0), Some(0.0));
        assert_eq!(reading(&records, WmbusQuantity::Volume, 1), Some(119.616));
        let dates: Vec<_> = records
            .iter()
            .filter_map(|record| record.text.as_deref())
            .collect();
        assert_eq!(dates, ["2021-12-31", "2019-12-31 13:30", "0000"]);
    }

    #[test]
    fn a_heat_meter_reads_bcd_energy_and_temperatures() {
        let bytes = [
            0x0C, 0x06, 0x27, 0x04, 0x85, 0x02, 0x0C, 0x14, 0x76, 0x34, 0x70, 0x00, 0x0A, 0x5A,
            0x43, 0x04, 0x0A, 0x5E, 0x15, 0x03, 0x0B, 0x2D, 0x00, 0x00, 0xF0,
        ];
        let records = parse(&bytes);
        assert_eq!(
            reading(&records, WmbusQuantity::Energy, 0),
            Some(2_850_427_000.0)
        );
        assert_eq!(reading(&records, WmbusQuantity::Volume, 0), Some(7_034.76));
        assert_eq!(
            reading(&records, WmbusQuantity::FlowTemperature, 0),
            Some(44.3)
        );
        assert_eq!(
            reading(&records, WmbusQuantity::ReturnTemperature, 0),
            Some(31.5)
        );
        assert_eq!(reading(&records, WmbusQuantity::Power, 0), Some(0.0));
    }

    fn variable_volume(lvar: &[u8]) -> Vec<WmbusRecord> {
        let mut bytes = vec![0x0D, 0x13];
        bytes.extend_from_slice(lvar);
        parse(&bytes)
    }

    #[test]
    fn variable_length_numbers_follow_their_lvar_code() {
        let value = |lvar: &[u8]| reading(&variable_volume(lvar), WmbusQuantity::Volume, 0);
        assert_eq!(value(&[0xC3, 0x56, 0x34, 0x12]), Some(123.456));
        assert_eq!(value(&[0xD2, 0x34, 0x12]), Some(-1.234));
        assert_eq!(value(&[0xE2, 0xD2, 0x04]), Some(1.234));
        let text = variable_volume(&[0x03, b'a', b'b', b'c']);
        assert_eq!(text[0].text.as_deref(), Some("cba"));
    }

    #[test]
    fn a_reserved_lvar_code_does_not_swallow_the_records_after_it() {
        let mut bytes = vec![0x0D, 0x13, 0xF5];
        bytes.resize(bytes.len() + 200, 0);
        assert!(parse(&bytes).is_empty());
        assert!(variable_volume(&[0xFF, 0x01, 0x02]).is_empty());
    }

    #[test]
    fn manufacturer_data_ends_the_records() {
        let records = parse(&[0x0B, 0x6E, 0x12, 0x00, 0x00, 0x0F, 0x01, 0x02]);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].quantity, WmbusQuantity::HeatCostUnits);
        assert_eq!(records[0].value, Some(12.0));
        assert!(parse(&[0x04, 0x13, 0x01]).is_empty());
    }
}
