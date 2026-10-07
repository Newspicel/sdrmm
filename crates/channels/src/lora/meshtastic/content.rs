use sdrmm_wire::{MeshtasticContent, MeshtasticNeighbor};

use super::ports;
use super::proto::{each_field, repeated_fixed32, repeated_varint};
use super::telemetry::telemetry;
use crate::datalink::hex;

const DEGREES_SCALE: f64 = 1e7;
const SNR_SCALE: f32 = 4.0;

pub(super) struct Data {
    pub(super) port: u32,
    pub(super) request_id: Option<u32>,
    pub(super) reply_id: Option<u32>,
    pub(super) content: MeshtasticContent,
}

pub(super) fn data(bytes: &[u8]) -> Option<Data> {
    let mut port = None;
    let mut payload: &[u8] = &[];
    let mut request_id = None;
    let mut reply_id = None;
    each_field(bytes, |field, value| {
        match field {
            1 => port = Some(value.uint32()?),
            2 => payload = value.bytes()?,
            3 | 9 => {
                value.varint()?;
            }
            4 | 5 | 8 => {
                value.fixed32()?;
            }
            6 => request_id = Some(value.fixed32()?),
            7 => reply_id = Some(value.fixed32()?),
            10 => {
                value.bytes()?;
            }
            _ => {}
        }
        Some(())
    })?;
    let port = port.filter(|port| *port != 0)?;
    Some(Data {
        port,
        request_id: request_id.filter(|id| *id != 0),
        reply_id: reply_id.filter(|id| *id != 0),
        content: content(port, payload),
    })
}

fn content(port: u32, payload: &[u8]) -> MeshtasticContent {
    let parsed = match port {
        ports::TEXT_MESSAGE => Some(MeshtasticContent::Text {
            text: String::from_utf8_lossy(payload).into_owned(),
        }),
        ports::POSITION => position(payload),
        ports::NODEINFO => node_info(payload),
        ports::ROUTING => routing(payload),
        ports::WAYPOINT => waypoint(payload),
        ports::TELEMETRY => telemetry(payload),
        ports::TRACEROUTE => traceroute(payload),
        ports::NEIGHBORINFO => neighbor_info(payload),
        ports::MAP_REPORT => map_report(payload),
        _ => None,
    };
    parsed.unwrap_or_else(|| MeshtasticContent::Data {
        bytes: hex(payload),
    })
}

fn degrees(raw: i32) -> f64 {
    f64::from(raw) / DEGREES_SCALE
}

fn position(bytes: &[u8]) -> Option<MeshtasticContent> {
    let (mut lat, mut lon, mut altitude_m, mut time) = (None, None, None, None);
    let (mut satellites, mut ground_speed_kmh, mut precision_bits) = (None, None, None);
    each_field(bytes, |field, value| {
        match field {
            1 => lat = Some(degrees(value.sfixed32()?)),
            2 => lon = Some(degrees(value.sfixed32()?)),
            3 => altitude_m = Some(value.int32()?),
            4 => time = Some(value.fixed32()?),
            15 => ground_speed_kmh = Some(value.uint32()?),
            19 => satellites = Some(value.uint32()?),
            23 => precision_bits = Some(value.uint32()?),
            _ => {}
        }
        Some(())
    })?;
    Some(MeshtasticContent::Position {
        lat,
        lon,
        altitude_m,
        time,
        satellites,
        ground_speed_kmh,
        precision_bits,
    })
}

fn node_info(bytes: &[u8]) -> Option<MeshtasticContent> {
    let (mut id, mut long_name, mut short_name) = (String::new(), String::new(), String::new());
    let (mut hw_model, mut role, mut licensed, mut public_key) = (None, None, false, None);
    each_field(bytes, |field, value| {
        match field {
            1 => id = value.text()?,
            2 => long_name = value.text()?,
            3 => short_name = value.text()?,
            5 => hw_model = Some(value.uint32()?),
            6 => licensed = value.boolean()?,
            7 => role = Some(value.uint32()?),
            8 => public_key = Some(value.bytes()?).filter(|key| !key.is_empty()).map(hex),
            _ => {}
        }
        Some(())
    })?;
    Some(MeshtasticContent::NodeInfo {
        id,
        long_name,
        short_name,
        hw_model,
        role,
        licensed,
        public_key,
    })
}

#[derive(Default)]
struct RouteDiscovery {
    route: Vec<u32>,
    snr_towards: Vec<f32>,
    route_back: Vec<u32>,
    snr_back: Vec<f32>,
}

fn snrs(raw: &[u64]) -> Vec<f32> {
    raw.iter()
        .map(|value| (*value as i32) as f32 / SNR_SCALE)
        .collect()
}

fn route_discovery(bytes: &[u8]) -> Option<RouteDiscovery> {
    let mut discovery = RouteDiscovery::default();
    let (mut snr_towards, mut snr_back) = (Vec::new(), Vec::new());
    each_field(bytes, |field, value| match field {
        1 => repeated_fixed32(value, &mut discovery.route),
        2 => repeated_varint(value, &mut snr_towards),
        3 => repeated_fixed32(value, &mut discovery.route_back),
        4 => repeated_varint(value, &mut snr_back),
        _ => Some(()),
    })?;
    discovery.snr_towards = snrs(&snr_towards);
    discovery.snr_back = snrs(&snr_back);
    Some(discovery)
}

fn routing(bytes: &[u8]) -> Option<MeshtasticContent> {
    let mut error = None;
    let mut route = Vec::new();
    each_field(bytes, |field, value| {
        match field {
            1 | 2 => route = route_discovery(value.bytes()?)?.route,
            3 => error = Some(value.uint32()?),
            _ => {}
        }
        Some(())
    })?;
    Some(MeshtasticContent::Routing {
        error: error.filter(|code| *code != 0).map(ports::routing_error),
        route,
    })
}

fn traceroute(bytes: &[u8]) -> Option<MeshtasticContent> {
    let discovery = route_discovery(bytes)?;
    Some(MeshtasticContent::Traceroute {
        route: discovery.route,
        snr_towards_db: discovery.snr_towards,
        route_back: discovery.route_back,
        snr_back_db: discovery.snr_back,
    })
}

fn neighbor(bytes: &[u8]) -> Option<MeshtasticNeighbor> {
    let mut neighbor = MeshtasticNeighbor {
        node: 0,
        snr_db: 0.0,
    };
    each_field(bytes, |field, value| {
        match field {
            1 => neighbor.node = value.uint32()?,
            2 => neighbor.snr_db = value.float()?,
            _ => {}
        }
        Some(())
    })?;
    Some(neighbor)
}

fn neighbor_info(bytes: &[u8]) -> Option<MeshtasticContent> {
    let mut node = 0;
    let mut neighbors = Vec::new();
    each_field(bytes, |field, value| {
        match field {
            1 => node = value.uint32()?,
            4 => neighbors.push(neighbor(value.bytes()?)?),
            _ => {}
        }
        Some(())
    })?;
    Some(MeshtasticContent::NeighborInfo { node, neighbors })
}

fn waypoint(bytes: &[u8]) -> Option<MeshtasticContent> {
    let (mut id, mut name, mut description) = (0, String::new(), String::new());
    let (mut lat, mut lon, mut expire) = (None, None, None);
    each_field(bytes, |field, value| {
        match field {
            1 => id = value.uint32()?,
            2 => lat = Some(degrees(value.sfixed32()?)),
            3 => lon = Some(degrees(value.sfixed32()?)),
            4 => expire = Some(value.uint32()?),
            6 => name = value.text()?,
            7 => description = value.text()?,
            _ => {}
        }
        Some(())
    })?;
    Some(MeshtasticContent::Waypoint {
        id,
        name,
        description,
        lat,
        lon,
        expire,
    })
}

fn map_report(bytes: &[u8]) -> Option<MeshtasticContent> {
    let (mut long_name, mut short_name, mut firmware_version) =
        (String::new(), String::new(), String::new());
    let (mut lat, mut lon, mut altitude_m, mut online_nodes) = (None, None, None, None);
    each_field(bytes, |field, value| {
        match field {
            1 => long_name = value.text()?,
            2 => short_name = value.text()?,
            5 => firmware_version = value.text()?,
            9 => lat = Some(degrees(value.sfixed32()?)),
            10 => lon = Some(degrees(value.sfixed32()?)),
            11 => altitude_m = Some(value.int32()?),
            13 => online_nodes = Some(value.uint32()?),
            _ => {}
        }
        Some(())
    })?;
    Some(MeshtasticContent::MapReport {
        long_name,
        short_name,
        firmware_version,
        lat,
        lon,
        altitude_m,
        online_nodes,
    })
}
