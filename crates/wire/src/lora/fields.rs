use super::{
    LoraFrame, LoraPayload, LorawanFrame, MeshcoreContent, MeshcoreNodeType, MeshcorePacket,
    MeshcoreRoute, MeshtasticContent, MeshtasticEncryption, MeshtasticMetric, MeshtasticNeighbor,
    MeshtasticPacket,
};
use crate::present::{
    Field, FieldValue, Unit, count, fields, flag, number, only_if, position, repaired, text,
    text_of,
};

impl LoraFrame {
    pub(super) fn fields(&self) -> Vec<Field> {
        let mut shown = fields(&[
            (
                "Protocol",
                self.decoded.as_ref().map(protocol).and_then(text_of),
            ),
            ("SF", text_of(format!("SF{}", self.spreading_factor))),
            ("Bandwidth", number(Some(self.bandwidth_hz), Unit::Hz, 0)),
            ("Coding rate", text_of(self.coding_rate.label())),
            ("Sync word", hex(self.sync_word.into(), 2)),
            ("Header", text_of(implicit(self.implicit_header))),
            ("Low data rate", only_if(self.low_data_rate)),
            ("IQ", text_of(inverted(self.inverted_iq))),
            ("Integrity", text_of(self.integrity.label())),
            ("FEC repaired", repaired(self.fec_corrected)),
            ("SNR", number(Some(self.snr_db), Unit::Db, 1)),
            (
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
        shown.extend(fields(&[("Payload", text(Some(&self.payload)))]));
        shown
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
        .map(|hz| crate::units::hertz(*hz))
        .collect();
    fields(&[
        ("Type", text_of(frame.message_type.label())),
        ("DevAddr", text(frame.dev_addr.as_deref())),
        ("DevEUI", text(frame.dev_eui.as_deref())),
        ("JoinEUI", text(frame.join_eui.as_deref())),
        ("FCnt", count(frame.f_cnt)),
        ("FPort", count(frame.f_port)),
        ("MIC", text(Some(&mic))),
        ("ADR", flag(frame.adr)),
        ("ADR ack request", flag(frame.adr_ack_req)),
        ("Ack", flag(frame.ack)),
        ("Pending", flag(frame.pending_or_class_b)),
        ("MAC commands", joined_text(&frame.mac_commands, ", ")),
        ("FRMPayload", text(frame.frm_payload.as_deref())),
        ("Decrypted", text(frame.decrypted.as_deref())),
        ("DevNonce", count(frame.dev_nonce)),
        ("JoinNonce", count(frame.join_nonce)),
        ("NetID", text(frame.net_id.as_deref())),
        ("RX1 DR offset", count(frame.rx1_dr_offset)),
        (
            "RX2 data rate",
            frame
                .rx2_data_rate
                .and_then(|rate| text_of(format!("DR{rate}"))),
        ),
        ("RX delay", number(frame.rx_delay_s, Unit::Seconds, 0)),
        ("Channels", joined_text(&channels, ", ")),
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
        ("From", text_of(MeshtasticPacket::node_id(packet.from))),
        ("To", text_of(MeshtasticPacket::node_id(packet.to))),
        ("Packet ID", hex(packet.id.into(), 8)),
        (
            "Channel",
            text(packet.channel.as_deref()).or_else(|| hex(packet.channel_hash.into(), 2)),
        ),
        ("Encryption", text_of(encryption(packet.encryption))),
        ("Port", text(port.as_deref())),
        ("Hops", text_of(hops)),
        ("Want ack", only_if(packet.want_ack)),
        ("Via MQTT", only_if(packet.via_mqtt)),
        ("Next hop", nonzero_hex(packet.next_hop)),
        ("Relay", nonzero_hex(packet.relay_node)),
        (
            "Request",
            packet.request_id.and_then(|id| hex(id.into(), 8)),
        ),
        ("Reply to", packet.reply_id.and_then(|id| hex(id.into(), 8))),
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
        MeshtasticContent::Text { text: message } => fields(&[("Text", text(Some(message)))]),
        MeshtasticContent::Position {
            lat,
            lon,
            altitude_m,
            time: fix,
            satellites,
            ground_speed_kmh,
            precision_bits,
        } => fields(&[
            ("Position", position(*lat, *lon)),
            ("Altitude", number(*altitude_m, Unit::Metres, 0)),
            ("Satellites", count(*satellites)),
            (
                "Ground speed",
                number(*ground_speed_kmh, Unit::KilometresPerHour, 0),
            ),
            ("Fix time", time(*fix)),
            (
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
            ("Node", text(Some(id))),
            ("Long name", text(Some(long_name))),
            ("Short name", text(Some(short_name))),
            ("Hardware", count(*hw_model)),
            ("Role", count(*role)),
            ("Licensed", only_if(*licensed)),
            ("Public key", text(public_key.as_deref())),
        ]),
        MeshtasticContent::Telemetry {
            kind,
            time: at,
            metrics,
        } => telemetry_fields(kind, *at, metrics),
        MeshtasticContent::Routing { error, route } => fields(&[
            ("Routing", text_of(error.as_deref().unwrap_or("ack"))),
            ("Route", joined_text(&node_ids(route), " → ")),
        ]),
        MeshtasticContent::Traceroute {
            route,
            snr_towards_db,
            route_back,
            snr_back_db,
        } => fields(&[
            (
                "Route",
                joined_text(&hops_with_snr(route, snr_towards_db), " → "),
            ),
            (
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
            ("Waypoint", text(Some(name))),
            ("Description", text(Some(description))),
            ("Position", position(*lat, *lon)),
            ("Expires", time(*expire)),
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
            ("Long name", text(Some(long_name))),
            ("Short name", text(Some(short_name))),
            ("Firmware", text(Some(firmware_version))),
            ("Position", position(*lat, *lon)),
            ("Altitude", number(*altitude_m, Unit::Metres, 0)),
            ("Online nodes", count(*online_nodes)),
        ]),
        MeshtasticContent::Data { bytes } => fields(&[("Data", text(Some(bytes)))]),
    }
}

fn telemetry_fields(kind: &str, at: Option<u32>, metrics: &[MeshtasticMetric]) -> Vec<Field> {
    let mut shown = fields(&[("Telemetry", text(Some(kind))), ("Time", time(at))]);
    shown.extend(metrics.iter().filter_map(|metric| {
        Some(Field {
            label: metric.name.clone(),
            value: number(Some(metric.value), Unit::None, 2)?,
        })
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
        ("Node", text_of(MeshtasticPacket::node_id(node))),
        ("Neighbors", joined_text(&heard, ", ")),
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
        ("Route", text_of(route(packet.route))),
        ("Type", text_of(packet.payload_type.label())),
        ("Version", count(Some(packet.version))),
        ("Transport codes", joined_text(&codes, ", ")),
        ("Path", joined_text(&packet.path, " → ")),
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
            ("Name", text(name.as_deref())),
            ("Node type", text_of(node_type(*kind))),
            ("Position", position(*lat, *lon)),
            ("Advertised", time(Some(*timestamp))),
            ("Public key", text(Some(public_key))),
            (
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
            ("Channel", channel(name.as_deref(), *channel_hash)),
            ("Sender", text(sender.as_deref())),
            ("Sent", time(*timestamp)),
            ("Text", text(message.as_deref())),
        ]),
        MeshcoreContent::GroupData {
            channel_hash,
            channel: name,
            data_type,
            data,
        } => fields(&[
            ("Channel", channel(name.as_deref(), *channel_hash)),
            ("Data type", data_type.and_then(|kind| hex(kind.into(), 4))),
            ("Data", text(data.as_deref())),
        ]),
        MeshcoreContent::Encrypted {
            destination,
            source,
            mac,
        } => fields(&[
            ("Source", text(Some(source))),
            ("Destination", text(Some(destination))),
            ("MAC", text(Some(mac))),
        ]),
        MeshcoreContent::AnonRequest {
            destination,
            public_key,
        } => fields(&[
            ("Destination", text(Some(destination))),
            ("Public key", text(Some(public_key))),
        ]),
        MeshcoreContent::Ack { checksum } => fields(&[("Checksum", hex((*checksum).into(), 8))]),
        MeshcoreContent::Trace {
            tag,
            auth_code,
            flags,
            hops,
        } => fields(&[
            ("Tag", hex((*tag).into(), 8)),
            ("Auth code", hex((*auth_code).into(), 8)),
            ("Flags", hex((*flags).into(), 2)),
            ("Hops", joined_text(hops, " → ")),
        ]),
        MeshcoreContent::Control { subtype, data } => fields(&[
            ("Subtype", hex((*subtype).into(), 2)),
            ("Data", text(Some(data))),
        ]),
        MeshcoreContent::Multipart {
            remaining,
            inner_type,
            data,
        } => fields(&[
            ("Remaining", count(Some(*remaining))),
            ("Inner type", hex((*inner_type).into(), 2)),
            ("Data", text(Some(data))),
        ]),
        MeshcoreContent::Raw { data } => fields(&[("Data", text(Some(data)))]),
    }
}

#[cfg(test)]
mod tests;
