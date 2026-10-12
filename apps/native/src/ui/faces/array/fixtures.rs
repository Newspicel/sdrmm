use sdrmm_wire::{
    device::Coherence,
    patch::{ARRAY_LANE_PORT, NodeBody, PatchEdge, PatchNode, PortRef, Position, stream_port},
    state::DeviceSet,
};
use serde_json::json;

pub fn radio(rx_streams: u32, coherence: Coherence) -> DeviceSet {
    let mut set: DeviceSet = serde_json::from_value(json!({
        "id": 1,
        "device": { "driver": "virtual", "key": "kraken5", "label": "KrakenSDR" },
        "capabilities": {
            "freq_ranges": [], "sample_rates": [], "gains": [], "antennas": [], "bandwidths": []
        },
        "settings": {},
        "status": "running",
        "channels": [],
        "overruns": 0
    }))
    .expect("device set");
    set.capabilities.rx_streams = rx_streams;
    set.capabilities.per_stream.tuning = true;
    set.capabilities.coherence = coherence;
    set
}

pub fn device(backend: &str, serial: &str) -> NodeBody {
    serde_json::from_value(json!({
        "kind": "device",
        "data": { "device": { "backend": backend, "serial": serial } }
    }))
    .expect("device body")
}

pub fn placed(id: &str, body: NodeBody, label: Option<&str>) -> PatchNode {
    PatchNode {
        id: id.to_owned(),
        body,
        position: Position { x: 100.0, y: 40.0 },
        size: None,
        label: label.map(str::to_owned),
    }
}

pub fn lane(device: &str, stream: u32, array: &str, slot: u32) -> PatchEdge {
    PatchEdge {
        from: PortRef {
            node: device.to_owned(),
            port: stream_port("iq", stream),
        },
        to: PortRef {
            node: array.to_owned(),
            port: stream_port(ARRAY_LANE_PORT, slot),
        },
    }
}
