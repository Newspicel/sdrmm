use sdrmm_wire::{MeshtasticContent, MeshtasticMetric};

use super::proto::{Value, each_field};

const DEVICE: &[&str] = &[
    "battery_level",
    "voltage",
    "channel_utilization",
    "air_util_tx",
    "uptime_seconds",
];

const ENVIRONMENT: &[&str] = &[
    "temperature",
    "relative_humidity",
    "barometric_pressure",
    "gas_resistance",
    "voltage",
    "current",
    "iaq",
    "distance",
    "lux",
    "white_lux",
    "ir_lux",
    "uv_lux",
    "wind_direction",
    "wind_speed",
    "weight",
    "wind_gust",
    "wind_lull",
    "radiation",
    "rainfall_1h",
    "rainfall_24h",
    "soil_moisture",
    "soil_temperature",
    "one_wire_temperature",
    "adc_voltage_ch0",
    "adc_voltage_ch1",
    "adc_voltage_ch2",
    "adc_voltage_ch3",
    "adc_voltage_ch4",
    "adc_voltage_ch5",
    "adc_voltage_ch6",
    "adc_voltage_ch7",
    "one_wire_temperature_ch0",
    "one_wire_temperature_ch1",
    "one_wire_temperature_ch2",
    "one_wire_temperature_ch3",
    "one_wire_temperature_ch4",
    "one_wire_temperature_ch5",
    "one_wire_temperature_ch6",
    "one_wire_temperature_ch7",
    "lightning_strike_count_1h",
    "lightning_distance_km",
];

const AIR_QUALITY: &[&str] = &[
    "pm10_standard",
    "pm25_standard",
    "pm100_standard",
    "pm10_environmental",
    "pm25_environmental",
    "pm100_environmental",
    "particles_03um",
    "particles_05um",
    "particles_10um",
    "particles_25um",
    "particles_50um",
    "particles_100um",
    "co2",
    "co2_temperature",
    "co2_humidity",
    "form_formaldehyde",
    "form_humidity",
    "form_temperature",
    "pm40_standard",
    "particles_40um",
    "pm_temperature",
    "pm_humidity",
    "pm_voc_idx",
    "pm_nox_idx",
    "particles_tps",
    "pm_status_flags",
];

const POWER: &[&str] = &[
    "ch1_voltage",
    "ch1_current",
    "ch2_voltage",
    "ch2_current",
    "ch3_voltage",
    "ch3_current",
    "ch4_voltage",
    "ch4_current",
    "ch5_voltage",
    "ch5_current",
    "ch6_voltage",
    "ch6_current",
    "ch7_voltage",
    "ch7_current",
    "ch8_voltage",
    "ch8_current",
];

const LOCAL_STATS: &[&str] = &[
    "uptime_seconds",
    "channel_utilization",
    "air_util_tx",
    "num_packets_tx",
    "num_packets_rx",
    "num_packets_rx_bad",
    "num_online_nodes",
    "num_total_nodes",
    "num_rx_dupe",
    "num_tx_relay",
    "num_tx_relay_canceled",
    "heap_total_bytes",
    "heap_free_bytes",
    "num_tx_dropped",
    "noise_floor",
];

const HEALTH: &[&str] = &["heart_bpm", "spO2", "temperature"];

const HOST: &[&str] = &[
    "uptime_seconds",
    "freemem_bytes",
    "diskfree1_bytes",
    "diskfree2_bytes",
    "diskfree3_bytes",
    "load1",
    "load5",
    "load15",
    "user_string",
];

const TRAFFIC_MANAGEMENT: &[&str] = &[
    "packets_inspected",
    "position_dedup_drops",
    "nodeinfo_cache_hits",
    "rate_limit_drops",
    "unknown_packet_drops",
    "hop_exhausted_packets",
    "router_hops_preserved",
];

const SOIL_WATER: &[&str] = &[
    "soil_ph",
    "ph",
    "electrical_conductivity",
    "salinity",
    "nitrogen",
    "phosphorus",
    "potassium",
    "dissolved_oxygen",
    "orp",
    "chemical_oxygen_demand",
    "turbidity",
    "nitrate",
    "ammonium",
    "biochemical_oxygen_demand",
    "solar_irradiance",
];

const KINDS: &[(u32, &str, &[&str])] = &[
    (2, "device", DEVICE),
    (3, "environment", ENVIRONMENT),
    (4, "air_quality", AIR_QUALITY),
    (5, "power", POWER),
    (6, "local_stats", LOCAL_STATS),
    (7, "health", HEALTH),
    (8, "host", HOST),
    (9, "traffic_management", TRAFFIC_MANAGEMENT),
    (11, "soil_water", SOIL_WATER),
];

const TIME: u32 = 1;

pub(super) fn telemetry(bytes: &[u8]) -> Option<MeshtasticContent> {
    let mut time = None;
    let mut variant = None;
    each_field(bytes, |field, value| {
        if field == TIME {
            time = Some(value.fixed32()?);
        } else if let Some((_, kind, names)) = KINDS.iter().find(|(known, ..)| *known == field) {
            variant = Some((*kind, *names, value.bytes()?));
        }
        Some(())
    })?;
    let (kind, names, body) = variant?;
    Some(MeshtasticContent::Telemetry {
        kind: kind.to_owned(),
        time,
        metrics: metrics(body, names)?,
    })
}

fn metrics(bytes: &[u8], names: &[&str]) -> Option<Vec<MeshtasticMetric>> {
    let mut metrics = Vec::new();
    each_field(bytes, |field, value| {
        let name = usize::try_from(field)
            .ok()
            .and_then(|field| field.checked_sub(1))
            .and_then(|index| names.get(index));
        if let (Some(name), Some(value)) = (name, number(value)) {
            metrics.push(MeshtasticMetric {
                name: (*name).to_owned(),
                value,
            });
        }
        Some(())
    })?;
    Some(metrics)
}

fn number(value: Value<'_>) -> Option<f64> {
    match value {
        Value::Varint(raw) => Some(raw.cast_signed() as f64),
        Value::Fixed32(raw) => Some(widen(f32::from_bits(raw))).filter(|value| value.is_finite()),
        Value::Fixed64(_) | Value::Bytes(_) => None,
    }
}

fn widen(value: f32) -> f64 {
    value
        .to_string()
        .parse()
        .unwrap_or_else(|_| f64::from(value))
}
