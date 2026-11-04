use super::{
    LoraFrame, LoraPayload, LorawanFrame, MeshcoreContent, MeshcoreNodeType, MeshcorePacket,
    MeshcoreRoute, MeshtasticContent, MeshtasticEncryption, MeshtasticMetric, MeshtasticNeighbor,
    MeshtasticPacket,
};
use sdrmm_wire::present::Field;
use sdrmm_wire::present::FieldValue;
use sdrmm_wire::present::Unit;
use sdrmm_wire::present::count;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::flag;
use sdrmm_wire::present::key_of;
use sdrmm_wire::present::number;
use sdrmm_wire::present::only_if;
use sdrmm_wire::present::position;
use sdrmm_wire::present::repaired;
use sdrmm_wire::present::text;
use sdrmm_wire::present::text_of;
use sdrmm_wire::present::unique_keys;

impl LoraFrame {
    pub(super) fn fields(&self) -> Vec<Field> {
        let mut shown = fields(&[
            (
                "protocol",
                "Protocol",
                self.decoded.as_ref().map(protocol).and_then(text_of),
            ),
            ("sf", "SF", text_of(format!("SF{}", self.spreading_factor))),
            (
                "bandwidth",
                "Bandwidth",
                number(Some(self.bandwidth_hz), Unit::Hz, 0),
            ),
            (
                "coding_rate",
                "Coding rate",
                text_of(self.coding_rate.label()),
            ),
            ("sync_word", "Sync word", hex(self.sync_word.into(), 2)),
            ("header", "Header", text_of(implicit(self.implicit_header))),
            (
                "low_data_rate",
                "Low data rate",
                only_if(self.low_data_rate),
            ),
            ("iq", "IQ", text_of(inverted(self.inverted_iq))),
            ("integrity", "Integrity", text_of(self.integrity.label())),
            ("fec_repaired", "FEC repaired", repaired(self.fec_corrected)),
            ("snr", "SNR", number(Some(self.snr_db), Unit::Db, 1)),
            (
                "frequency_error",
                "Frequency error",
                number(Some(self.frequency_error_hz), Unit::Hz, 0),
            ),
        ]);
        shown.extend(match &self.decoded {
            Some(LoraPayload::Lorawan(frame)) => lorawan_fields(frame),
            Some(LoraPayload::Meshtastic(packet)) => meshtastic_fields(packet),
            Some(LoraPayload::Meshcore(packet)) => meshcore_fields(packet),
            None => Vec::new(),
        });
        shown.extend(fields(&[("payload", "Payload", text(Some(&self.payload)))]));
        unique_keys(shown)
    }
}

fn protocol(decoded: &LoraPayload) -> &'static str {
    match decoded {
        LoraPayload::Lorawan(_) => "LoRaWAN",
        LoraPayload::Meshtastic(_) => "Meshtastic",
        LoraPayload::Meshcore(_) => "MeshCore",
    }
}

const fn implicit(implicit: bool) -> &'static str {
    if implicit { "implicit" } else { "explicit" }
}

const fn inverted(inverted: bool) -> &'static str {
    if inverted { "inverted" } else { "normal" }
}

fn hex(value: u64, width: usize) -> Option<FieldValue> {
    text_of(format!("0x{value:0width$X}"))
}

fn time(epoch_s: Option<u32>) -> Option<FieldValue> {
    let epoch_s = epoch_s.filter(|epoch_s| *epoch_s > 0)?;
    jiff::Timestamp::from_second(epoch_s.into())
        .ok()
        .map(|at| FieldValue::Time(at.to_string()))
}

fn joined_text(parts: &[String], separator: &str) -> Option<FieldValue> {
    text(Some(&parts.join(separator)))
}

fn lorawan_fields(frame: &LorawanFrame) -> Vec<Field> {
    let mic = match frame.mic_ok {
        Some(true) => format!("{} ok", frame.mic),
        Some(false) => format!("{} failed", frame.mic),
        None => frame.mic.clone(),
    };
    let channels: Vec<String> = frame
        .cf_list_hz
        .iter()
        .map(|hz| sdrmm_wire::units::hertz(*hz))
        .collect();
    fields(&[
        ("type", "Type", text_of(frame.message_type.label())),
        ("devaddr", "DevAddr", text(frame.dev_addr.as_deref())),
        ("deveui", "DevEUI", text(frame.dev_eui.as_deref())),
        ("joineui", "JoinEUI", text(frame.join_eui.as_deref())),
        ("fcnt", "FCnt", count(frame.f_cnt)),
        ("fport", "FPort", count(frame.f_port)),
        ("mic", "MIC", text(Some(&mic))),
        ("adr", "ADR", flag(frame.adr)),
        (
            "adr_ack_request",
            "ADR ack request",
            flag(frame.adr_ack_req),
        ),
        ("ack", "Ack", flag(frame.ack)),
        ("pending", "Pending", flag(frame.pending_or_class_b)),
        (
            "mac_commands",
            "MAC commands",
            joined_text(&frame.mac_commands, ", "),
        ),
        (
            "frmpayload",
            "FRMPayload",
            text(frame.frm_payload.as_deref()),
        ),
        ("decrypted", "Decrypted", text(frame.decrypted.as_deref())),
        ("devnonce", "DevNonce", count(frame.dev_nonce)),
        ("joinnonce", "JoinNonce", count(frame.join_nonce)),
        ("netid", "NetID", text(frame.net_id.as_deref())),
        ("rx1_dr_offset", "RX1 DR offset", count(frame.rx1_dr_offset)),
        (
            "rx2_data_rate",
            "RX2 data rate",
            frame
                .rx2_data_rate
                .and_then(|rate| text_of(format!("DR{rate}"))),
        ),
        (
            "rx_delay",
            "RX delay",
            number(frame.rx_delay_s, Unit::Seconds, 0),
        ),
        ("channels", "Channels", joined_text(&channels, ", ")),
    ])
}

fn meshtastic_fields(packet: &MeshtasticPacket) -> Vec<Field> {
    let hops = format!(
        "{} of {}",
        packet.hop_start.saturating_sub(packet.hop_limit),
        packet.hop_start
    );
    let port = packet
        .port_name
        .clone()
        .or_else(|| packet.port.map(|port| port.to_string()));
    let mut shown = fields(&[
        (
            "from",
            "From",
            text_of(MeshtasticPacket::node_id(packet.from)),
        ),
        ("to", "To", text_of(MeshtasticPacket::node_id(packet.to))),
        ("packet_id", "Packet ID", hex(packet.id.into(), 8)),
        (
            "channel",
            "Channel",
            text(packet.channel.as_deref()).or_else(|| hex(packet.channel_hash.into(), 2)),
        ),
        (
            "encryption",
            "Encryption",
            text_of(encryption(packet.encryption)),
        ),
        ("port", "Port", text(port.as_deref())),
        ("hops", "Hops", text_of(hops)),
        ("want_ack", "Want ack", only_if(packet.want_ack)),
        ("via_mqtt", "Via MQTT", only_if(packet.via_mqtt)),
        ("next_hop", "Next hop", nonzero_hex(packet.next_hop)),
        ("relay", "Relay", nonzero_hex(packet.relay_node)),
        (
            "request",
            "Request",
            packet.request_id.and_then(|id| hex(id.into(), 8)),
        ),
        (
            "reply_to",
            "Reply to",
            packet.reply_id.and_then(|id| hex(id.into(), 8)),
        ),
    ]);
    shown.extend(
        packet
            .content
            .as_ref()
            .map(meshtastic_content_fields)
            .unwrap_or_default(),
    );
    shown
}

fn nonzero_hex(value: u8) -> Option<FieldValue> {
    (value != 0).then(|| hex(value.into(), 2)).flatten()
}

const fn encryption(encryption: MeshtasticEncryption) -> &'static str {
    match encryption {
        MeshtasticEncryption::Open => "open",
        MeshtasticEncryption::Channel => "channel",
        MeshtasticEncryption::Pki => "pki",
        MeshtasticEncryption::Unknown => "unknown",
    }
}

fn node_ids(nodes: &[u32]) -> Vec<String> {
    nodes
        .iter()
        .copied()
        .map(MeshtasticPacket::node_id)
        .collect()
}

fn meshtastic_content_fields(content: &MeshtasticContent) -> Vec<Field> {
    match content {
        MeshtasticContent::Text { text: message } => {
            fields(&[("text", "Text", text(Some(message)))])
        }
        MeshtasticContent::Position {
            lat,
            lon,
            altitude_m,
            time: fix,
            satellites,
            ground_speed_kmh,
            precision_bits,
        } => fields(&[
            ("position", "Position", position(*lat, *lon)),
            ("altitude", "Altitude", number(*altitude_m, Unit::Metres, 0)),
            ("satellites", "Satellites", count(*satellites)),
            (
                "ground_speed",
                "Ground speed",
                number(*ground_speed_kmh, Unit::KilometresPerHour, 0),
            ),
            ("fix_time", "Fix time", time(*fix)),
            (
                "precision",
                "Precision",
                precision_bits.and_then(|bits| text_of(format!("{bits} bits"))),
            ),
        ]),
        MeshtasticContent::NodeInfo {
            id,
            long_name,
            short_name,
            hw_model,
            role,
            licensed,
            public_key,
        } => fields(&[
            ("node", "Node", text(Some(id))),
            ("long_name", "Long name", text(Some(long_name))),
            ("short_name", "Short name", text(Some(short_name))),
            ("hardware", "Hardware", count(*hw_model)),
            ("role", "Role", count(*role)),
            ("licensed", "Licensed", only_if(*licensed)),
            ("public_key", "Public key", text(public_key.as_deref())),
        ]),
        MeshtasticContent::Telemetry {
            kind,
            time: at,
            metrics,
        } => telemetry_fields(kind, *at, metrics),
        MeshtasticContent::Routing { error, route } => fields(&[
            (
                "routing",
                "Routing",
                text_of(error.as_deref().unwrap_or("ack")),
            ),
            ("route", "Route", joined_text(&node_ids(route), " → ")),
        ]),
        MeshtasticContent::Traceroute {
            route,
            snr_towards_db,
            route_back,
            snr_back_db,
        } => fields(&[
            (
                "route",
                "Route",
                joined_text(&hops_with_snr(route, snr_towards_db), " → "),
            ),
            (
                "route_back",
                "Route back",
                joined_text(&hops_with_snr(route_back, snr_back_db), " → "),
            ),
        ]),
        MeshtasticContent::NeighborInfo { node, neighbors } => neighbor_fields(*node, neighbors),
        MeshtasticContent::Waypoint {
            name,
            description,
            lat,
            lon,
            expire,
            ..
        } => fields(&[
            ("waypoint", "Waypoint", text(Some(name))),
            ("description", "Description", text(Some(description))),
            ("position", "Position", position(*lat, *lon)),
            ("expires", "Expires", time(*expire)),
        ]),
        MeshtasticContent::MapReport {
            long_name,
            short_name,
            firmware_version,
            lat,
            lon,
            altitude_m,
            online_nodes,
        } => fields(&[
            ("long_name", "Long name", text(Some(long_name))),
            ("short_name", "Short name", text(Some(short_name))),
            ("firmware", "Firmware", text(Some(firmware_version))),
            ("position", "Position", position(*lat, *lon)),
            ("altitude", "Altitude", number(*altitude_m, Unit::Metres, 0)),
            ("online_nodes", "Online nodes", count(*online_nodes)),
        ]),
        MeshtasticContent::Data { bytes } => fields(&[("data", "Data", text(Some(bytes)))]),
    }
}

fn telemetry_fields(kind: &str, at: Option<u32>, metrics: &[MeshtasticMetric]) -> Vec<Field> {
    let mut shown = fields(&[
        ("telemetry", "Telemetry", text(Some(kind))),
        ("time", "Time", time(at)),
    ]);
    shown.extend(metrics.iter().filter_map(|metric| {
        Some(Field::new(
            key_of(&metric.name),
            metric.name.clone(),
            number(Some(metric.value), Unit::None, 2)?,
        ))
    }));
    shown
}

fn neighbor_fields(node: u32, neighbors: &[MeshtasticNeighbor]) -> Vec<Field> {
    let heard: Vec<String> = neighbors
        .iter()
        .map(|neighbor| {
            format!(
                "{} {:+.1} dB",
                MeshtasticPacket::node_id(neighbor.node),
                neighbor.snr_db
            )
        })
        .collect();
    fields(&[
        ("node", "Node", text_of(MeshtasticPacket::node_id(node))),
        ("neighbors", "Neighbors", joined_text(&heard, ", ")),
    ])
}

fn hops_with_snr(route: &[u32], snr_db: &[f32]) -> Vec<String> {
    route
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let id = MeshtasticPacket::node_id(*node);
            match snr_db.get(index) {
                Some(snr) => format!("{id} ({snr:+.1} dB)"),
                None => id,
            }
        })
        .collect()
}

fn meshcore_fields(packet: &MeshcorePacket) -> Vec<Field> {
    let codes: Vec<String> = packet
        .transport_codes
        .iter()
        .map(|code| format!("0x{code:04X}"))
        .collect();
    let mut shown = fields(&[
        ("route", "Route", text_of(route(packet.route))),
        ("type", "Type", text_of(packet.payload_type.label())),
        ("version", "Version", count(Some(packet.version))),
        (
            "transport_codes",
            "Transport codes",
            joined_text(&codes, ", "),
        ),
        ("path", "Path", joined_text(&packet.path, " → ")),
    ]);
    shown.extend(meshcore_content_fields(&packet.content));
    shown
}

const fn route(route: MeshcoreRoute) -> &'static str {
    match route {
        MeshcoreRoute::TransportFlood => "flood with transport codes",
        MeshcoreRoute::Flood => "flood",
        MeshcoreRoute::Direct => "direct",
        MeshcoreRoute::TransportDirect => "direct with transport codes",
    }
}

const fn node_type(node_type: MeshcoreNodeType) -> &'static str {
    match node_type {
        MeshcoreNodeType::None => "none",
        MeshcoreNodeType::Chat => "chat",
        MeshcoreNodeType::Repeater => "repeater",
        MeshcoreNodeType::Room => "room",
        MeshcoreNodeType::Sensor => "sensor",
        MeshcoreNodeType::Other => "other",
    }
}

fn channel(channel: Option<&str>, hash: u8) -> Option<FieldValue> {
    text(channel).or_else(|| hex(hash.into(), 2))
}

fn meshcore_content_fields(content: &MeshcoreContent) -> Vec<Field> {
    match content {
        MeshcoreContent::Advert {
            public_key,
            timestamp,
            node_type: kind,
            name,
            lat,
            lon,
            signature_ok,
        } => fields(&[
            ("name", "Name", text(name.as_deref())),
            ("node_type", "Node type", text_of(node_type(*kind))),
            ("position", "Position", position(*lat, *lon)),
            ("advertised", "Advertised", time(Some(*timestamp))),
            ("public_key", "Public key", text(Some(public_key))),
            (
                "signature",
                "Signature",
                text_of(if *signature_ok { "verified" } else { "failed" }),
            ),
        ]),
        MeshcoreContent::GroupText {
            channel_hash,
            channel: name,
            timestamp,
            sender,
            text: message,
        } => fields(&[
            (
                "channel",
                "Channel",
                channel(name.as_deref(), *channel_hash),
            ),
            ("sender", "Sender", text(sender.as_deref())),
            ("sent", "Sent", time(*timestamp)),
            ("text", "Text", text(message.as_deref())),
        ]),
        MeshcoreContent::GroupData {
            channel_hash,
            channel: name,
            data_type,
            data,
        } => fields(&[
            (
                "channel",
                "Channel",
                channel(name.as_deref(), *channel_hash),
            ),
            (
                "data_type",
                "Data type",
                data_type.and_then(|kind| hex(kind.into(), 4)),
            ),
            ("data", "Data", text(data.as_deref())),
        ]),
        MeshcoreContent::Encrypted {
            destination,
            source,
            mac,
        } => fields(&[
            ("source", "Source", text(Some(source))),
            ("destination", "Destination", text(Some(destination))),
            ("mac", "MAC", text(Some(mac))),
        ]),
        MeshcoreContent::AnonRequest {
            destination,
            public_key,
        } => fields(&[
            ("destination", "Destination", text(Some(destination))),
            ("public_key", "Public key", text(Some(public_key))),
        ]),
        MeshcoreContent::Ack { checksum } => {
            fields(&[("checksum", "Checksum", hex((*checksum).into(), 8))])
        }
        MeshcoreContent::Trace {
            tag,
            auth_code,
            flags,
            hops,
        } => fields(&[
            ("tag", "Tag", hex((*tag).into(), 8)),
            ("auth_code", "Auth code", hex((*auth_code).into(), 8)),
            ("flags", "Flags", hex((*flags).into(), 2)),
            ("hops", "Hops", joined_text(hops, " → ")),
        ]),
        MeshcoreContent::Control { subtype, data } => fields(&[
            ("subtype", "Subtype", hex((*subtype).into(), 2)),
            ("data", "Data", text(Some(data))),
        ]),
        MeshcoreContent::Multipart {
            remaining,
            inner_type,
            data,
        } => fields(&[
            ("remaining", "Remaining", count(Some(*remaining))),
            ("inner_type", "Inner type", hex((*inner_type).into(), 2)),
            ("data", "Data", text(Some(data))),
        ]),
        MeshcoreContent::Raw { data } => fields(&[("data", "Data", text(Some(data)))]),
    }
}

#[cfg(test)]
mod tests;
