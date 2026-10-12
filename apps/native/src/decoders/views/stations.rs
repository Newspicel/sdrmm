use std::{cmp::Reverse, collections::HashMap, sync::Arc};

use sdrmm_wire::{
    ble::BleAdvert,
    decode::{DecodedRecord, DecoderEvent},
    geo::{LatLon, distance_m},
    lora::{
        LoraFrame, LoraIntegrity, LoraPayload, LoraProtocol, MeshcoreContent, MeshtasticContent,
    },
    remote_id::{RemoteIdFrame, RemoteIdMessage, UaType},
    units,
    weather::{AprsWeather, RadiosondeFrame, SondeType},
};

use super::{Records, at_ms};
use crate::decoders::text::serde_name;

fn chronological(records: &[Arc<DecodedRecord>]) -> Vec<&Arc<DecodedRecord>> {
    let mut ordered: Vec<&Arc<DecodedRecord>> = records.iter().rev().collect();
    ordered.sort_by_key(|record| at_ms(record));
    ordered
}

#[must_use]
pub fn latest_record(records: &[Arc<DecodedRecord>]) -> Option<Arc<DecodedRecord>> {
    let mut latest: Option<&Arc<DecodedRecord>> = None;
    for record in records {
        if latest.is_none_or(|held| at_ms(record) > at_ms(held)) {
            latest = Some(record);
        }
    }
    latest.cloned()
}

pub const BURST_DROP_M: f64 = 200.0;

#[derive(Clone, Debug, PartialEq)]
pub struct RadiosondeStation {
    pub serial: String,
    pub sonde: SondeType,
    pub frame: Option<u32>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub altitude_m: Option<f64>,
    pub climb_ms: Option<f64>,
    pub temperature_c: Option<f32>,
    pub humidity_pct: Option<f32>,
    pub pressure_hpa: Option<f32>,
    pub max_altitude_m: Option<f64>,
    pub burst: bool,
    pub frames: u64,
    pub at: String,
    pub at_ms: i64,
}

fn highest<T: PartialOrd>(a: Option<T>, b: Option<T>) -> Option<T> {
    match (a, b) {
        (Some(a), Some(b)) => Some(if b > a { b } else { a }),
        (a, b) => a.or(b),
    }
}

fn lowest<T: PartialOrd>(a: Option<T>, b: Option<T>) -> Option<T> {
    match (a, b) {
        (Some(a), Some(b)) => Some(if b < a { b } else { a }),
        (a, b) => a.or(b),
    }
}

fn next_sonde(
    previous: Option<&RadiosondeStation>,
    frame: &RadiosondeFrame,
    record: &DecodedRecord,
) -> RadiosondeStation {
    let altitude_m = frame
        .altitude_m
        .or_else(|| previous.and_then(|p| p.altitude_m));
    let max_altitude_m = highest(previous.and_then(|p| p.max_altitude_m), altitude_m);
    let fell = altitude_m
        .zip(max_altitude_m)
        .is_some_and(|(now, top)| top - now > BURST_DROP_M);
    RadiosondeStation {
        serial: frame.serial.clone(),
        sonde: frame.sonde,
        frame: frame.frame.or_else(|| previous.and_then(|p| p.frame)),
        lat: frame.lat.or_else(|| previous.and_then(|p| p.lat)),
        lon: frame.lon.or_else(|| previous.and_then(|p| p.lon)),
        altitude_m,
        climb_ms: frame.climb_ms.or_else(|| previous.and_then(|p| p.climb_ms)),
        temperature_c: frame
            .temperature_c
            .or_else(|| previous.and_then(|p| p.temperature_c)),
        humidity_pct: frame
            .humidity_pct
            .or_else(|| previous.and_then(|p| p.humidity_pct)),
        pressure_hpa: frame
            .pressure_hpa
            .or_else(|| previous.and_then(|p| p.pressure_hpa)),
        max_altitude_m,
        burst: previous.is_some_and(|p| p.burst) || fell,
        frames: previous.map_or(0, |p| p.frames) + 1,
        at: record.at.clone(),
        at_ms: at_ms(record),
    }
}

fn sonde_of(record: &DecodedRecord) -> Option<&RadiosondeFrame> {
    match &record.event {
        DecoderEvent::Radiosonde(frame) => Some(frame),
        _ => None,
    }
}

#[must_use]
pub fn radiosonde_stations(records: &[Arc<DecodedRecord>]) -> Vec<RadiosondeStation> {
    let mut sondes: HashMap<String, RadiosondeStation> = HashMap::new();
    for record in chronological(records) {
        let Some(frame) = sonde_of(record) else {
            continue;
        };
        let next = next_sonde(sondes.get(&frame.serial), frame, record);
        sondes.insert(frame.serial.clone(), next);
    }
    newest_first(sondes.into_values().collect(), |sonde| sonde.at_ms)
}

#[must_use]
pub fn radiosonde_log(records: &[Arc<DecodedRecord>], serial: &str, limit: usize) -> Records {
    records
        .iter()
        .filter(|record| sonde_of(record).is_some_and(|frame| frame.serial == serial))
        .take(limit)
        .cloned()
        .collect()
}

fn newest_first<T>(mut items: Vec<T>, at: impl Fn(&T) -> i64) -> Vec<T> {
    items.sort_by_key(|item| Reverse(at(item)));
    items
}

#[must_use]
pub const fn ua_type_label(kind: UaType) -> &'static str {
    match kind {
        UaType::None => "-",
        UaType::Aeroplane => "Aeroplane",
        UaType::Rotorcraft => "Multirotor",
        UaType::Gyroplane => "Gyroplane",
        UaType::HybridLift => "VTOL",
        UaType::Ornithopter => "Ornithopter",
        UaType::Glider => "Glider",
        UaType::Kite => "Kite",
        UaType::FreeBalloon => "Balloon",
        UaType::CaptiveBalloon => "Captive balloon",
        UaType::Airship => "Airship",
        UaType::Parachute => "Parachute",
        UaType::Rocket => "Rocket",
        UaType::TetheredAircraft => "Tethered",
        UaType::GroundObstacle => "Obstacle",
        UaType::Other => "Other",
    }
}

fn message_slot(message: &RemoteIdMessage) -> String {
    match message {
        RemoteIdMessage::BasicId { id_type, .. } => format!("basic_id:{}", serde_name(id_type)),
        RemoteIdMessage::Authentication { page, .. } => format!("authentication:{page}"),
        RemoteIdMessage::Unknown { message_type, .. } => format!("unknown:{message_type}"),
        RemoteIdMessage::Location(_) => "location".to_owned(),
        RemoteIdMessage::SelfId { .. } => "self_id".to_owned(),
        RemoteIdMessage::System(_) => "system".to_owned(),
        RemoteIdMessage::OperatorId { .. } => "operator_id".to_owned(),
    }
}

#[must_use]
pub fn with_remote_id_state(previous: &RemoteIdFrame, next: &RemoteIdFrame) -> RemoteIdFrame {
    let fresh: Vec<String> = next.messages.iter().map(message_slot).collect();
    let kept = previous
        .messages
        .iter()
        .filter(|message| !fresh.contains(&message_slot(message)))
        .cloned();
    let mut merged = next.clone();
    merged.uas_id = next.uas_id.clone().or_else(|| previous.uas_id.clone());
    merged.ssid = next.ssid.clone().or_else(|| previous.ssid.clone());
    merged.messages.extend(kept);
    merged
}

#[must_use]
pub fn with_lora_position(previous: &LoraFrame, next: &LoraFrame) -> LoraFrame {
    if next.position().is_some() || previous.position().is_none() {
        return next.clone();
    }
    let mut merged = next.clone();
    merged.decoded.clone_from(&previous.decoded);
    merged
}

#[must_use]
pub fn ua_type_of(frame: &RemoteIdFrame) -> Option<UaType> {
    frame.messages.iter().find_map(|message| match message {
        RemoteIdMessage::BasicId { ua_type, .. } => Some(*ua_type),
        _ => None,
    })
}

#[must_use]
pub fn operator_of(frame: &RemoteIdFrame) -> Option<String> {
    frame.messages.iter().find_map(|message| match message {
        RemoteIdMessage::OperatorId { operator_id, .. } if !operator_id.is_empty() => {
            Some(operator_id.clone())
        }
        _ => None,
    })
}

#[must_use]
pub fn pilot_of(frame: &RemoteIdFrame) -> Option<(f64, f64)> {
    let system = frame.system()?;
    system.operator_lat.zip(system.operator_lon)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Drone {
    pub key: String,
    pub kind: &'static str,
    pub height_m: Option<f32>,
    pub speed_mps: Option<f32>,
    pub pilot_m: Option<f64>,
    pub operator: Option<String>,
    pub links: Vec<&'static str>,
    pub level_dbfs: f32,
    pub frames: u64,
    pub at: String,
    pub at_ms: i64,
}

fn point((lat, lon): (f64, f64)) -> LatLon {
    LatLon { lat, lon }
}

fn next_drone(
    previous: Option<&Drone>,
    frame: &RemoteIdFrame,
    next: &RemoteIdFrame,
    record: &DecodedRecord,
) -> Drone {
    let location = frame.location();
    let link = next.transport.label();
    let mut links = previous.map(|p| p.links.clone()).unwrap_or_default();
    if !links.contains(&link) {
        links.push(link);
    }
    Drone {
        key: frame.station(),
        kind: ua_type_of(frame).map_or("-", ua_type_label),
        height_m: location.and_then(|l| l.height_m),
        speed_mps: location.and_then(|l| l.speed_mps),
        pilot_m: frame
            .position()
            .zip(pilot_of(frame))
            .map(|(drone, pilot)| distance_m(point(drone), point(pilot))),
        operator: operator_of(frame),
        links,
        level_dbfs: next.level_dbfs,
        frames: previous.map_or(0, |p| p.frames) + 1,
        at: record.at.clone(),
        at_ms: at_ms(record),
    }
}

#[must_use]
pub fn remote_id_drones(records: &[Arc<DecodedRecord>]) -> Vec<Drone> {
    let mut states: HashMap<String, (RemoteIdFrame, Drone)> = HashMap::new();
    for record in chronological(records) {
        let DecoderEvent::RemoteId(next) = &record.event else {
            continue;
        };
        let key = next.station();
        let previous = states.get(&key);
        let frame = previous.map_or_else(
            || next.clone(),
            |(held, _)| with_remote_id_state(held, next),
        );
        let drone = next_drone(previous.map(|(_, drone)| drone), &frame, next, record);
        states.insert(key, (frame, drone));
    }
    newest_first(
        states.into_values().map(|(_, drone)| drone).collect(),
        |drone| drone.at_ms,
    )
}

#[derive(Clone, Debug, PartialEq)]
pub struct BleDevice {
    pub key: String,
    pub name: Option<String>,
    pub vendor: Option<String>,
    pub beacon: Option<&'static str>,
    pub kind: &'static str,
    pub channel: Option<u8>,
    pub level_dbfs: f32,
    pub heard: u64,
    pub at: String,
    pub at_ms: i64,
}

#[must_use]
pub fn ble_key(advert: &BleAdvert) -> String {
    advert.station().unwrap_or_else(|| {
        let label = advert.pdu.label();
        match advert.adi {
            Some(adi) => format!("{label} SID {} DID {:X}", adi.set, adi.data_id),
            None => format!(
                "{label} {}",
                advert.data.chars().take(12).collect::<String>()
            ),
        }
    })
}

#[must_use]
pub fn ble_devices(records: &[Arc<DecodedRecord>]) -> Vec<BleDevice> {
    let mut devices: HashMap<String, BleDevice> = HashMap::new();
    for record in chronological(records) {
        let DecoderEvent::Ble(advert) = &record.event else {
            continue;
        };
        let key = ble_key(advert);
        let previous = devices.get(&key);
        let device = BleDevice {
            key: key.clone(),
            name: advert
                .name
                .clone()
                .or_else(|| previous.and_then(|p| p.name.clone())),
            vendor: advert
                .vendor()
                .map(str::to_owned)
                .or_else(|| previous.and_then(|p| p.vendor.clone())),
            beacon: advert
                .beacon
                .as_ref()
                .map(|beacon| beacon.label())
                .or_else(|| previous.and_then(|p| p.beacon)),
            kind: advert.pdu.label(),
            channel: advert.channel,
            level_dbfs: advert.level_dbfs,
            heard: previous.map_or(0, |p| p.heard) + 1 + u64::from(advert.repeats),
            at: record.at.clone(),
            at_ms: at_ms(record),
        };
        devices.insert(key, device);
    }
    newest_first(devices.into_values().collect(), |device| device.at_ms)
}

#[must_use]
pub fn lora_protocol(frame: &LoraFrame) -> &'static str {
    match &frame.decoded {
        None => LoraProtocol::Raw.label(),
        Some(LoraPayload::Lorawan(_)) => LoraProtocol::Lorawan.label(),
        Some(LoraPayload::Meshtastic(_)) => LoraProtocol::Meshtastic.label(),
        Some(LoraPayload::Meshcore(_)) => LoraProtocol::Meshcore.label(),
    }
}

#[must_use]
pub fn lora_name(frame: &LoraFrame) -> Option<String> {
    match frame.decoded.as_ref()? {
        LoraPayload::Meshtastic(packet) => match packet.content.as_ref()? {
            MeshtasticContent::NodeInfo { long_name, .. }
            | MeshtasticContent::MapReport { long_name, .. } => {
                Some(long_name.trim().to_owned()).filter(|name| !name.is_empty())
            }
            _ => None,
        },
        LoraPayload::Meshcore(packet) => match &packet.content {
            MeshcoreContent::Advert { name, .. } => name.clone(),
            _ => None,
        },
        LoraPayload::Lorawan(_) => None,
    }
}

#[must_use]
pub fn lora_message(frame: &LoraFrame) -> String {
    let summary = frame.summary();
    let head = format!(
        "SF{} {} · ",
        frame.spreading_factor,
        units::hertz(frame.bandwidth_hz)
    );
    let body = summary.strip_prefix(&head).unwrap_or(&summary);
    let tail = format!(" · {}", frame.integrity.label());
    let body = if frame.integrity == LoraIntegrity::CrcOk {
        body
    } else {
        body.strip_suffix(&tail).unwrap_or(body)
    };
    body.to_owned()
}

#[derive(Clone, Debug, PartialEq)]
pub struct LoraStation {
    pub key: String,
    pub protocol: &'static str,
    pub name: Option<String>,
    pub message: String,
    pub snr_db: f32,
    pub frames: u64,
    pub at: String,
    pub at_ms: i64,
}

#[must_use]
pub fn lora_stations(records: &[Arc<DecodedRecord>]) -> Vec<LoraStation> {
    let mut stations: HashMap<String, LoraStation> = HashMap::new();
    for record in chronological(records) {
        let DecoderEvent::Lora(frame) = &record.event else {
            continue;
        };
        let Some(key) = frame.station() else {
            continue;
        };
        let previous = stations.get(&key);
        let station = LoraStation {
            key: key.clone(),
            protocol: lora_protocol(frame),
            name: lora_name(frame).or_else(|| previous.and_then(|p| p.name.clone())),
            message: lora_message(frame),
            snr_db: frame.snr_db,
            frames: previous.map_or(0, |p| p.frames) + 1,
            at: record.at.clone(),
            at_ms: at_ms(record),
        };
        stations.insert(key, station);
    }
    newest_first(stations.into_values().collect(), |station| station.at_ms)
}

#[derive(Clone, Debug, PartialEq)]
pub struct AprsWeatherStation {
    pub source: String,
    pub weather: AprsWeather,
    pub min_temperature_c: Option<f32>,
    pub max_temperature_c: Option<f32>,
    pub max_gust_ms: Option<f32>,
    pub reports: u64,
    pub at: String,
    pub at_ms: i64,
}

#[must_use]
pub fn overlay_weather(previous: &AprsWeather, next: &AprsWeather) -> AprsWeather {
    AprsWeather {
        wind_dir_deg: next.wind_dir_deg.or(previous.wind_dir_deg),
        wind_speed_ms: next.wind_speed_ms.or(previous.wind_speed_ms),
        wind_gust_ms: next.wind_gust_ms.or(previous.wind_gust_ms),
        temperature_c: next.temperature_c.or(previous.temperature_c),
        rain_1h_mm: next.rain_1h_mm.or(previous.rain_1h_mm),
        rain_24h_mm: next.rain_24h_mm.or(previous.rain_24h_mm),
        rain_midnight_mm: next.rain_midnight_mm.or(previous.rain_midnight_mm),
        humidity_pct: next.humidity_pct.or(previous.humidity_pct),
        pressure_hpa: next.pressure_hpa.or(previous.pressure_hpa),
        luminosity_wm2: next.luminosity_wm2.or(previous.luminosity_wm2),
        snow_24h_mm: next.snow_24h_mm.or(previous.snow_24h_mm),
    }
}

fn next_weather(
    previous: Option<&AprsWeatherStation>,
    source: &str,
    weather: &AprsWeather,
    record: &DecodedRecord,
) -> AprsWeatherStation {
    let temperature = weather.temperature_c;
    AprsWeatherStation {
        source: source.to_owned(),
        weather: previous.map_or_else(|| weather.clone(), |p| overlay_weather(&p.weather, weather)),
        min_temperature_c: lowest(previous.and_then(|p| p.min_temperature_c), temperature),
        max_temperature_c: highest(previous.and_then(|p| p.max_temperature_c), temperature),
        max_gust_ms: highest(previous.and_then(|p| p.max_gust_ms), weather.wind_gust_ms),
        reports: previous.map_or(0, |p| p.reports) + 1,
        at: record.at.clone(),
        at_ms: at_ms(record),
    }
}

#[must_use]
pub fn aprs_weather_stations(records: &[Arc<DecodedRecord>]) -> Vec<AprsWeatherStation> {
    let mut stations: HashMap<String, AprsWeatherStation> = HashMap::new();
    for record in chronological(records) {
        let DecoderEvent::Aprs(packet) = &record.event else {
            continue;
        };
        let Some(weather) = &packet.weather else {
            continue;
        };
        let next = next_weather(
            stations.get(&packet.source),
            &packet.source,
            weather,
            record,
        );
        stations.insert(packet.source.clone(), next);
    }
    newest_first(stations.into_values().collect(), |station| station.at_ms)
}

#[must_use]
pub fn wind(weather: &AprsWeather) -> Option<String> {
    let speed = crate::decoders::text::metres_per_second(weather.wind_speed_ms.map(f64::from))?;
    Some(match weather.wind_dir_deg {
        Some(from) => format!("{speed} from {from}°"),
        None => speed,
    })
}

#[cfg(test)]
mod tests;
