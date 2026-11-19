//! Horus Binary v3: the `Telemetry` sequence of horusdemodlib's `HorusBinaryV3.asn1`, in
//! unaligned PER.

use sdrmm_dsp::coding::per::Per;

use super::model::{HorusFrame, HorusSensor};
use super::packet::hex;

const CALLSIGN: &[u8] = b"-/0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const SENSOR_NAME: &[u8] = b"-0123456789abcdefghijklmnopqrstuvwxyz";
const SENSOR_TEXT: &[u8] = b" +-./0123456789=ABCDEFGHIJKLMNOPQRSTUVWXYZ_abcdefghijklmnopqrstuvwxyz";
const OPTIONAL_FIELDS: usize = 11;
const NO_TIME: i64 = -1;
const NO_ALTITUDE: i64 = -1_000;
const DEGREE_SCALE: f64 = 100_000.0;
const SECONDS_PER_DAY: i64 = 86_400;
const MAX_SENSORS: i64 = 4;
const MAX_VALUES: i64 = 4;
const MAX_COUNTS: i64 = 8;
const FLAGS: usize = 8;

/// Which optional `Telemetry` fields are present, in the order the sequence lists them.
struct Present([bool; OPTIONAL_FIELDS]);

impl Present {
    fn read(per: &mut Per) -> Option<Self> {
        let mut flags = [false; OPTIONAL_FIELDS];
        for flag in &mut flags {
            *flag = per.bit()? == 1;
        }
        Some(Self(flags))
    }

    fn has(&self, field: Optional) -> bool {
        self.0[field as usize]
    }
}

#[derive(Clone, Copy)]
enum Optional {
    ExtraSensors,
    Speed,
    Satellites,
    Ascent,
    Pressure,
    Temperatures,
    Humidity,
    MilliVolts,
    Counts,
    PowerSave,
    CustomData,
}

/// The telemetry in `body`, the bytes after the CRC; `None` when it breaks the schema.
pub(crate) fn parse(body: &[u8]) -> Option<HorusFrame> {
    let mut per = Per::new(body);
    let _extended = per.bit()?;
    let present = Present::read(&mut per)?;
    let callsign = per.restricted(1, 15, CALLSIGN)?;
    let sequence = u16::try_from(per.constrained(0, 65_535)?).ok()?;
    let mut frame = HorusFrame::new(3, callsign, sequence);
    let seconds = per.constrained(NO_TIME, SECONDS_PER_DAY)?;
    frame.time = (seconds != NO_TIME).then(|| time_of_day(seconds));
    let lat = per.constrained(-9_000_000, 9_000_000)? as f64 / DEGREE_SCALE;
    let lon = per.constrained(-18_000_000, 18_000_000)? as f64 / DEGREE_SCALE;
    frame.place(lat, lon);
    let altitude = per.constrained(NO_ALTITUDE, 50_000)?;
    frame.altitude_m = (altitude != NO_ALTITUDE).then_some(altitude as f64);
    if present.has(Optional::ExtraSensors) {
        frame.sensors = sensors(&mut per)?;
    }
    optional_fields(&mut per, &present, &mut frame)?;
    Some(frame)
}

fn optional_fields(per: &mut Per, present: &Present, frame: &mut HorusFrame) -> Option<()> {
    if present.has(Optional::Speed) {
        frame.speed_kmh = Some(per.constrained(0, 512)? as f64);
    }
    if present.has(Optional::Satellites) {
        frame.satellites = Some(per.constrained(0, 31)? as u8);
    }
    if present.has(Optional::Ascent) {
        frame.climb_ms = Some(per.constrained(-32_767, 32_767)? as f64 / 100.0);
    }
    if present.has(Optional::Pressure) {
        frame.pressure_hpa = Some(per.constrained(0, 12_000)? as f32 / 10.0);
    }
    if present.has(Optional::Temperatures) {
        temperatures(per, frame)?;
    }
    if present.has(Optional::Humidity) {
        frame.humidity_pct = Some(per.constrained(0, 100)? as f32);
    }
    if present.has(Optional::MilliVolts) {
        milli_volts(per, frame)?;
    }
    if present.has(Optional::Counts) {
        let n = per.constrained(1, MAX_COUNTS)?;
        let counts = (0..n)
            .map(|_| per.unconstrained().map(|count| count.to_string()))
            .collect::<Option<Vec<_>>>()?;
        frame.sensors.push(sensor("counts", counts.join(" ")));
    }
    if present.has(Optional::PowerSave) {
        let state = per.constrained(0, 5)?;
        frame.sensors.push(sensor("gnss power", power_save(state)));
    }
    if present.has(Optional::CustomData) {
        frame.custom = Some(hex(&per.octets(0, 255)?));
    }
    Some(())
}

/// Tenths of a degree: internal, external, then two the payload names itself.
fn temperatures(per: &mut Per, frame: &mut HorusFrame) -> Option<()> {
    let present = [per.bit()?, per.bit()?, per.bit()?, per.bit()?];
    for (slot, name) in ["internal", "external", "temperature 1", "temperature 2"]
        .into_iter()
        .enumerate()
    {
        if present[slot] == 0 {
            continue;
        }
        let celsius = per.constrained(-1_023, 1_023)? as f32 / 10.0;
        match slot {
            0 => frame.temperature_c = Some(celsius),
            1 => frame.external_temperature_c = Some(celsius),
            _ => frame.sensors.push(sensor(name, format!("{celsius:.1} °C"))),
        }
    }
    Some(())
}

/// Millivolts: battery, solar, then two the payload names itself.
fn milli_volts(per: &mut Per, frame: &mut HorusFrame) -> Option<()> {
    let present = [per.bit()?, per.bit()?, per.bit()?, per.bit()?];
    for (slot, name) in ["battery", "solar", "voltage 1", "voltage 2"]
        .into_iter()
        .enumerate()
    {
        if present[slot] == 0 {
            continue;
        }
        let volts = per.constrained(0, 16_383)? as f32 / 1_000.0;
        if slot == 0 {
            frame.battery_v = Some(volts);
        } else {
            frame.sensors.push(sensor(name, format!("{volts:.3} V")));
        }
    }
    Some(())
}

fn sensors(per: &mut Per) -> Option<Vec<HorusSensor>> {
    let n = per.constrained(1, MAX_SENSORS)?;
    (0..n).map(|index| extra_sensor(per, index)).collect()
}

fn extra_sensor(per: &mut Per, index: i64) -> Option<HorusSensor> {
    let has_name = per.bit()? == 1;
    let has_values = per.bit()? == 1;
    let name = if has_name {
        per.restricted(1, 20, SENSOR_NAME)?
    } else {
        format!("sensor {}", index + 1)
    };
    let value = if has_values {
        sensor_values(per)?
    } else {
        String::new()
    };
    Some(sensor(&name, value))
}

fn sensor_values(per: &mut Per) -> Option<String> {
    match per.constrained(0, 3)? {
        0 => per.restricted(0, 255, SENSOR_TEXT),
        1 => {
            let n = per.constrained(1, MAX_VALUES)?;
            let values = (0..n)
                .map(|_| per.unconstrained().map(|value| value.to_string()))
                .collect::<Option<Vec<_>>>()?;
            Some(values.join(" "))
        }
        2 => {
            let n = per.constrained(1, MAX_VALUES)?;
            let values = (0..n)
                .map(|_| per.real().map(|value| value.to_string()))
                .collect::<Option<Vec<_>>>()?;
            Some(values.join(" "))
        }
        _ => {
            let flags = (0..FLAGS)
                .map(|_| per.bit().map(|bit| char::from(b'0' + bit)))
                .collect::<Option<String>>()?;
            Some(flags)
        }
    }
}

fn sensor(name: &str, value: String) -> HorusSensor {
    HorusSensor {
        name: name.to_owned(),
        value,
    }
}

fn power_save(state: i64) -> String {
    match state {
        0 => "not active",
        1 => "enabled",
        2 => "acquisition",
        3 => "tracking",
        4 => "optimised",
        _ => "inactive",
    }
    .to_owned()
}

fn time_of_day(seconds: i64) -> String {
    let seconds = seconds.rem_euclid(SECONDS_PER_DAY);
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdrmm_dsp::coding::bits::BitWriter;

    fn put_signed(bits: &mut BitWriter, value: i64, lo: i64, width: u32) {
        bits.put((value - lo) as u32, width);
    }

    fn callsign(bits: &mut BitWriter, text: &str) {
        bits.put(text.len() as u32 - 1, 4);
        for byte in text.bytes() {
            let index = CALLSIGN
                .iter()
                .position(|&c| c == byte)
                .expect("in alphabet");
            bits.put(index as u32, 6);
        }
    }

    #[test]
    fn reads_the_mandatory_fields_and_a_battery() {
        let mut bits = BitWriter::with_capacity(32);
        bits.bit(false);
        for field in 0..OPTIONAL_FIELDS {
            bits.bit(
                field == Optional::Satellites as usize || field == Optional::MilliVolts as usize,
            );
        }
        callsign(&mut bits, "VK5ARG-1");
        bits.put(4_321, 16);
        put_signed(&mut bits, 45_296, -1, 17);
        put_signed(&mut bits, -3_490_000, -9_000_000, 25);
        put_signed(&mut bits, 13_860_000, -18_000_000, 26);
        put_signed(&mut bits, 25_000, -1_000, 16);
        bits.put(11, 5);
        bits.put(0b1000, 4);
        bits.put(3_300, 14);
        let frame = parse(bits.bytes()).expect("parses");
        assert_eq!(frame.payload, "VK5ARG-1");
        assert_eq!(frame.sequence, 4_321);
        assert_eq!(frame.time.as_deref(), Some("12:34:56"));
        assert_eq!(frame.lat, Some(-34.9));
        assert_eq!(frame.lon, Some(138.6));
        assert_eq!(frame.altitude_m, Some(25_000.0));
        assert_eq!(frame.satellites, Some(11));
        assert_eq!(frame.battery_v, Some(3.3));
    }

    #[test]
    fn no_fix_and_no_time_stay_empty() {
        let mut bits = BitWriter::with_capacity(32);
        bits.put(0, 12);
        callsign(&mut bits, "A");
        bits.put(0, 16);
        put_signed(&mut bits, -1, -1, 17);
        put_signed(&mut bits, 0, -9_000_000, 25);
        put_signed(&mut bits, 0, -18_000_000, 26);
        put_signed(&mut bits, -1_000, -1_000, 16);
        let frame = parse(bits.bytes()).expect("parses");
        assert_eq!(frame.time, None);
        assert_eq!((frame.lat, frame.altitude_m), (None, None));
    }

    #[test]
    fn a_truncated_body_is_refused() {
        assert!(parse(&[0x00, 0x10]).is_none());
    }
}
