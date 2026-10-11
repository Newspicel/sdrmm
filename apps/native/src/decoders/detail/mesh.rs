use std::borrow::Cow;

use sdrmm_wire::{
    ble::{BleAdvert, BleBeacon, BleFlags},
    lora::{
        LoraFrame, LoraPayload, LorawanFrame, MeshcoreContent, MeshcorePacket, MeshcoreRoute,
        MeshtasticContent, MeshtasticPacket,
    },
    remote_id::{RemoteIdFrame, RemoteIdMessage, UaStatus},
    units::hertz,
};

use super::{EventDetail, Row, detail, some};
use crate::decoders::{
    text::{flag, hex, position, serde_name, signed},
    views::{lora_protocol, operator_of, pilot_of, ua_type_label, ua_type_of},
};

type Rows = Vec<(&'static str, Option<String>)>;

fn named(rows: Rows) -> Vec<Row> {
    rows.into_iter()
        .map(|(label, value)| (Cow::Borrowed(label), value))
        .collect()
}

fn number(value: Option<impl ToString>) -> Option<String> {
    value.map(|value| value.to_string())
}

fn hexed(value: u32, width: usize) -> Option<String> {
    some(hex(u64::from(value), width))
}

fn unix_time(epoch_s: Option<u32>) -> Option<String> {
    let epoch_s = epoch_s.filter(|s| *s != 0)?;
    jiff::Timestamp::from_second(i64::from(epoch_s))
        .ok()
        .map(|stamp| stamp.to_string())
}

pub fn lora(frame: &LoraFrame) -> EventDetail {
    let mut rows: Rows = vec![
        (
            "Protocol",
            frame
                .decoded
                .as_ref()
                .map(|_| lora_protocol(frame).to_owned()),
        ),
        (
            "Spreading factor",
            some(format!("SF{}", frame.spreading_factor)),
        ),
        ("Bandwidth", some(hertz(frame.bandwidth_hz))),
        ("Coding rate", some(frame.coding_rate.label())),
        ("Sync word", hexed(u32::from(frame.sync_word), 2)),
        (
            "Header",
            some(if frame.implicit_header {
                "implicit"
            } else {
                "explicit"
            }),
        ),
        (
            "Low data rate",
            frame.low_data_rate.then(|| "on".to_owned()),
        ),
        (
            "IQ",
            some(if frame.inverted_iq {
                "inverted"
            } else {
                "normal"
            }),
        ),
        ("Integrity", some(frame.integrity.label())),
        (
            "FEC repaired",
            (frame.fec_corrected > 0).then(|| frame.fec_corrected.to_string()),
        ),
        (
            "SNR",
            some(format!("{} dB", signed(f64::from(frame.snr_db), 1))),
        ),
        (
            "Frequency error",
            some(format!(
                "{} Hz",
                signed(f64::from(frame.frequency_error_hz), 0)
            )),
        ),
    ];
    let mut metrics: Vec<Row> = Vec::new();
    let body = match &frame.decoded {
        Some(LoraPayload::Lorawan(decoded)) => {
            rows.extend(lorawan_rows(decoded));
            None
        }
        Some(LoraPayload::Meshtastic(packet)) => {
            rows.extend(meshtastic_rows(packet));
            metrics = telemetry_rows(packet);
            match &packet.content {
                Some(MeshtasticContent::Text { text }) => some(text),
                _ => None,
            }
        }
        Some(LoraPayload::Meshcore(packet)) => {
            rows.extend(meshcore_rows(packet));
            match &packet.content {
                MeshcoreContent::GroupText { text, .. } => text.clone(),
                _ => None,
            }
        }
        None => None,
    };
    let payload = named(vec![("Payload", some(&frame.payload))]);
    detail(named(rows).into_iter().chain(metrics).chain(payload), body)
}

fn telemetry_rows(packet: &MeshtasticPacket) -> Vec<Row> {
    let Some(MeshtasticContent::Telemetry { metrics, .. }) = &packet.content else {
        return Vec::new();
    };
    metrics
        .iter()
        .map(|metric| {
            (
                Cow::Owned(metric.name.clone()),
                some(metric.value.to_string()),
            )
        })
        .collect()
}

fn lorawan_rows(frame: &LorawanFrame) -> Rows {
    let mic = match frame.mic_ok {
        None => frame.mic.clone(),
        Some(ok) => format!("{} {}", frame.mic, if ok { "ok" } else { "failed" }),
    };
    let channels: Vec<String> = frame.cf_list_hz.iter().map(|hz| hertz(*hz)).collect();
    vec![
        ("Type", some(frame.message_type.label())),
        ("DevAddr", frame.dev_addr.clone()),
        ("DevEUI", frame.dev_eui.clone()),
        ("JoinEUI", frame.join_eui.clone()),
        ("FCnt", number(frame.f_cnt)),
        ("FPort", number(frame.f_port)),
        ("MIC", some(mic)),
        ("ADR", flag(frame.adr)),
        ("ADR ack request", flag(frame.adr_ack_req)),
        ("Ack", flag(frame.ack)),
        ("Pending", flag(frame.pending_or_class_b)),
        ("MAC commands", some(frame.mac_commands.join(", "))),
        ("FRMPayload", frame.frm_payload.clone()),
        ("Decrypted", frame.decrypted.clone()),
        ("DevNonce", number(frame.dev_nonce)),
        ("JoinNonce", number(frame.join_nonce)),
        ("NetID", frame.net_id.clone()),
        ("RX1 DR offset", number(frame.rx1_dr_offset)),
        (
            "RX2 data rate",
            frame.rx2_data_rate.map(|dr| format!("DR{dr}")),
        ),
        ("RX delay", frame.rx_delay_s.map(|s| format!("{s} s"))),
        ("Channels", some(channels.join(", "))),
    ]
}

fn node(id: u32) -> String {
    MeshtasticPacket::node_id(id)
}

fn meshtastic_rows(packet: &MeshtasticPacket) -> Rows {
    let mut rows = vec![
        ("From", some(node(packet.from))),
        ("To", some(node(packet.to))),
        ("Packet ID", hexed(packet.id, 8)),
        (
            "Channel",
            packet
                .channel
                .clone()
                .or_else(|| hexed(u32::from(packet.channel_hash), 2)),
        ),
        ("Encryption", some(serde_name(&packet.encryption))),
        (
            "Port",
            packet.port_name.clone().or_else(|| number(packet.port)),
        ),
        (
            "Hops",
            some(format!(
                "{} of {}",
                packet.hop_start.saturating_sub(packet.hop_limit),
                packet.hop_start
            )),
        ),
        ("Want ack", packet.want_ack.then(|| "yes".to_owned())),
        ("Via MQTT", packet.via_mqtt.then(|| "yes".to_owned())),
        (
            "Next hop",
            (packet.next_hop != 0).then(|| hex(u64::from(packet.next_hop), 2)),
        ),
        (
            "Relay",
            (packet.relay_node != 0).then(|| hex(u64::from(packet.relay_node), 2)),
        ),
        ("Request", packet.request_id.and_then(|id| hexed(id, 8))),
        ("Reply to", packet.reply_id.and_then(|id| hexed(id, 8))),
    ];
    if let Some(content) = &packet.content {
        rows.extend(meshtastic_content_rows(content));
    }
    rows
}

fn hops_with_snr(route: &[u32], snr_db: &[f32]) -> String {
    route
        .iter()
        .enumerate()
        .map(|(index, hop)| match snr_db.get(index) {
            Some(snr) => format!("{} ({} dB)", node(*hop), signed(f64::from(*snr), 1)),
            None => node(*hop),
        })
        .collect::<Vec<_>>()
        .join(" → ")
}

fn metres_whole(value: Option<i32>) -> Option<String> {
    value.map(|m| format!("{m} m"))
}

fn meshtastic_content_rows(content: &MeshtasticContent) -> Rows {
    match content {
        MeshtasticContent::Text { .. } => Vec::new(),
        MeshtasticContent::Position {
            lat,
            lon,
            altitude_m,
            time,
            satellites,
            ground_speed_kmh,
            precision_bits,
        } => vec![
            ("Position", position(*lat, *lon)),
            ("Altitude", metres_whole(*altitude_m)),
            ("Satellites", number(*satellites)),
            (
                "Ground speed",
                ground_speed_kmh.map(|v| format!("{v} km/h")),
            ),
            ("Fix time", unix_time(*time)),
            ("Precision", precision_bits.map(|v| format!("{v} bits"))),
        ],
        MeshtasticContent::NodeInfo {
            id,
            long_name,
            short_name,
            hw_model,
            role,
            licensed,
            public_key,
        } => vec![
            ("Node", some(id)),
            ("Long name", some(long_name)),
            ("Short name", some(short_name)),
            ("Hardware", number(*hw_model)),
            ("Role", number(*role)),
            ("Licensed", licensed.then(|| "yes".to_owned())),
            ("Public key", public_key.clone()),
        ],
        MeshtasticContent::Telemetry { kind, time, .. } => {
            vec![("Telemetry", some(kind)), ("Time", unix_time(*time))]
        }
        MeshtasticContent::Routing { error, route } => vec![
            (
                "Routing",
                some(error.clone().unwrap_or_else(|| "ack".to_owned())),
            ),
            ("Route", some(hops_with_snr(route, &[]))),
        ],
        MeshtasticContent::Traceroute {
            route,
            snr_towards_db,
            route_back,
            snr_back_db,
        } => vec![
            ("Route", some(hops_with_snr(route, snr_towards_db))),
            ("Route back", some(hops_with_snr(route_back, snr_back_db))),
        ],
        MeshtasticContent::NeighborInfo {
            node: of,
            neighbors,
        } => vec![
            ("Node", some(node(*of))),
            (
                "Neighbors",
                some(
                    neighbors
                        .iter()
                        .map(|n| format!("{} {} dB", node(n.node), signed(f64::from(n.snr_db), 1)))
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
            ),
        ],
        MeshtasticContent::Waypoint {
            name,
            description,
            lat,
            lon,
            expire,
            ..
        } => vec![
            ("Waypoint", some(name)),
            ("Description", some(description)),
            ("Position", position(*lat, *lon)),
            ("Expires", unix_time(*expire)),
        ],
        MeshtasticContent::MapReport {
            long_name,
            short_name,
            firmware_version,
            lat,
            lon,
            altitude_m,
            online_nodes,
        } => vec![
            ("Long name", some(long_name)),
            ("Short name", some(short_name)),
            ("Firmware", some(firmware_version)),
            ("Position", position(*lat, *lon)),
            ("Altitude", metres_whole(*altitude_m)),
            ("Online nodes", number(*online_nodes)),
        ],
        MeshtasticContent::Data { bytes } => vec![("Data", some(bytes))],
    }
}

const fn meshcore_route(route: MeshcoreRoute) -> &'static str {
    match route {
        MeshcoreRoute::TransportFlood => "flood with transport codes",
        MeshcoreRoute::Flood => "flood",
        MeshcoreRoute::Direct => "direct",
        MeshcoreRoute::TransportDirect => "direct with transport codes",
    }
}

fn meshcore_rows(packet: &MeshcorePacket) -> Rows {
    let codes: Vec<String> = packet
        .transport_codes
        .iter()
        .map(|code| hex(u64::from(*code), 4))
        .collect();
    let mut rows = vec![
        ("Route", some(meshcore_route(packet.route))),
        ("Type", some(packet.payload_type.label())),
        ("Version", some(packet.version.to_string())),
        ("Transport codes", some(codes.join(", "))),
        ("Path", some(packet.path.join(" → "))),
    ];
    rows.extend(meshcore_content_rows(&packet.content));
    rows
}

fn channel_of(channel: Option<&String>, hash: u8) -> Option<String> {
    channel.cloned().or_else(|| hexed(u32::from(hash), 2))
}

fn meshcore_content_rows(content: &MeshcoreContent) -> Rows {
    match content {
        MeshcoreContent::Advert {
            public_key,
            timestamp,
            node_type,
            name,
            lat,
            lon,
            signature_ok,
        } => vec![
            ("Name", name.clone()),
            ("Node type", some(serde_name(node_type))),
            ("Position", position(*lat, *lon)),
            ("Advertised", unix_time(Some(*timestamp))),
            ("Public key", some(public_key)),
            (
                "Signature",
                some(if *signature_ok { "verified" } else { "failed" }),
            ),
        ],
        MeshcoreContent::GroupText {
            channel_hash,
            channel,
            timestamp,
            sender,
            ..
        } => vec![
            ("Channel", channel_of(channel.as_ref(), *channel_hash)),
            ("Sender", sender.clone()),
            ("Sent", unix_time(*timestamp)),
        ],
        MeshcoreContent::GroupData {
            channel_hash,
            channel,
            data_type,
            data,
        } => vec![
            ("Channel", channel_of(channel.as_ref(), *channel_hash)),
            ("Data type", data_type.and_then(|t| hexed(u32::from(t), 4))),
            ("Data", data.clone()),
        ],
        MeshcoreContent::Encrypted {
            destination,
            source,
            mac,
        } => vec![
            ("Source", some(source)),
            ("Destination", some(destination)),
            ("MAC", some(mac)),
        ],
        MeshcoreContent::AnonRequest {
            destination,
            public_key,
        } => vec![
            ("Destination", some(destination)),
            ("Public key", some(public_key)),
        ],
        MeshcoreContent::Ack { checksum } => vec![("Checksum", hexed(*checksum, 8))],
        MeshcoreContent::Trace {
            tag,
            auth_code,
            flags,
            hops,
        } => vec![
            ("Tag", hexed(*tag, 8)),
            ("Auth code", hexed(*auth_code, 8)),
            ("Flags", hexed(u32::from(*flags), 2)),
            ("Hops", some(hops.join(" → "))),
        ],
        MeshcoreContent::Control { subtype, data } => vec![
            ("Subtype", hexed(u32::from(*subtype), 2)),
            ("Data", some(data)),
        ],
        MeshcoreContent::Multipart {
            remaining,
            inner_type,
            data,
        } => vec![
            ("Remaining", some(remaining.to_string())),
            ("Inner type", hexed(u32::from(*inner_type), 2)),
            ("Data", some(data)),
        ],
        MeshcoreContent::Raw { data } => vec![("Data", some(data))],
    }
}

const fn ua_status(status: UaStatus) -> &'static str {
    match status {
        UaStatus::Undeclared => "-",
        UaStatus::Ground => "Ground",
        UaStatus::Airborne => "Airborne",
        UaStatus::Emergency => "Emergency",
        UaStatus::SystemFailure => "RID failure",
        UaStatus::Reserved => "Reserved",
    }
}

fn length_m(value: Option<f32>, digits: usize) -> Option<String> {
    value.map(|v| format!("{v:.digits$} m"))
}

fn self_text(frame: &RemoteIdFrame) -> Option<String> {
    frame.messages.iter().find_map(|message| match message {
        RemoteIdMessage::SelfId { text, .. } if !text.is_empty() => Some(text.clone()),
        _ => None,
    })
}

pub fn remote_id(frame: &RemoteIdFrame) -> EventDetail {
    let location = frame.location();
    let system = frame.system();
    let pages = frame
        .messages
        .iter()
        .filter(|message| matches!(message, RemoteIdMessage::Authentication { .. }))
        .count();
    let class = system.and_then(|s| s.classification).map(|c| {
        let tier = c
            .class
            .map_or_else(String::new, |class| format!(" C{class}"));
        format!("{}{tier}", serde_name(&c.category))
    });
    let rows: Rows = vec![
        ("UAS ID", frame.uas_id.clone()),
        (
            "Type",
            ua_type_of(frame).map(|kind| ua_type_label(kind).to_owned()),
        ),
        ("Status", location.map(|l| ua_status(l.status).to_owned())),
        ("Position", location.and_then(|l| position(l.lat, l.lon))),
        ("Height", length_m(location.and_then(|l| l.height_m), 1)),
        (
            "Altitude",
            length_m(location.and_then(|l| l.geodetic_altitude_m), 1),
        ),
        (
            "Pressure altitude",
            length_m(location.and_then(|l| l.pressure_altitude_m), 1),
        ),
        (
            "Speed",
            location
                .and_then(|l| l.speed_mps)
                .map(|v| format!("{v:.2} m/s")),
        ),
        (
            "Track",
            location
                .and_then(|l| l.track_deg)
                .map(|v| format!("{v:.0}°")),
        ),
        (
            "Climb",
            location
                .and_then(|l| l.vertical_speed_mps)
                .map(|v| format!("{} m/s", signed(f64::from(v), 1))),
        ),
        (
            "Accuracy",
            length_m(location.and_then(|l| l.horizontal_accuracy_m), 0),
        ),
        (
            "Time",
            location
                .and_then(|l| l.seconds_after_hour)
                .map(|s| format!("{s:.1} s past the hour")),
        ),
        (
            "Pilot",
            pilot_of(frame).and_then(|(lat, lon)| position(Some(lat), Some(lon))),
        ),
        (
            "Pilot altitude",
            length_m(system.and_then(|s| s.operator_altitude_m), 0),
        ),
        ("EU class", class),
        ("Operator", operator_of(frame)),
        ("Auth pages", (pages > 0).then(|| pages.to_string())),
        ("Link", some(frame.transport.label())),
        ("PHY", some(frame.phy.label())),
        ("Address", some(&frame.address)),
        ("Channel", number(frame.channel)),
        ("SSID", frame.ssid.clone()),
        ("Counter", number(frame.counter)),
        ("Level", some(format!("{:.1} dBFS", frame.level_dbfs))),
        (
            "Rejected",
            (frame.rejected > 0).then(|| frame.rejected.to_string()),
        ),
    ];
    detail(named(rows), self_text(frame))
}

fn ble_flags(flags: BleFlags) -> String {
    let named: Vec<&str> = [
        (flags.limited, "limited"),
        (flags.general, "general"),
        (flags.le_only, "LE only"),
    ]
    .into_iter()
    .filter_map(|(on, name)| on.then_some(name))
    .collect();
    if named.is_empty() {
        "none".to_owned()
    } else {
        named.join(", ")
    }
}

#[must_use]
pub fn ble_beacon_text(beacon: &BleBeacon) -> String {
    match beacon {
        BleBeacon::Ibeacon {
            uuid,
            major,
            minor,
            measured_dbm,
        } => format!("{uuid} {major}/{minor} · {measured_dbm} dBm at 1 m"),
        BleBeacon::AltBeacon { id, measured_dbm } => format!("{id} · {measured_dbm} dBm at 1 m"),
        BleBeacon::EddystoneUid {
            namespace,
            instance,
            ..
        } => format!("{namespace} {instance}"),
        BleBeacon::EddystoneUrl { url, .. } => url.clone(),
        BleBeacon::EddystoneTlm {
            battery_mv,
            temperature_c,
            adverts,
            uptime_s,
        } => crate::decoders::text::join([
            battery_mv.map_or_else(String::new, |mv| format!("{:.2} V", f64::from(mv) / 1000.0)),
            temperature_c.map_or_else(String::new, |t| format!("{t:.1} °C")),
            format!("{adverts} adverts"),
            format!("up {} s", uptime_s.round()),
        ]),
        BleBeacon::EddystoneEid { eid, .. } => eid.clone(),
        BleBeacon::FindMy { maintained } => if *maintained {
            "owner nearby"
        } else {
            "separated"
        }
        .to_owned(),
        BleBeacon::ExposureNotification { identifier } => identifier.clone(),
        BleBeacon::FastPair { model } => format!("model {model}"),
    }
}

fn ble_payload_rows(advert: &BleAdvert) -> Vec<Row> {
    let makers = advert.manufacturer.iter().map(|maker| {
        let label = maker
            .company
            .clone()
            .unwrap_or_else(|| format!("Company 0x{:04x}", maker.company_id));
        (Cow::Owned(label), some(&maker.data))
    });
    let services = advert.services.iter().map(|service| {
        let label = service
            .name
            .clone()
            .unwrap_or_else(|| format!("Service {}", service.uuid));
        let value = service.data.as_ref().map_or_else(
            || service.uuid.clone(),
            |data| format!("{} {data}", service.uuid),
        );
        (Cow::Owned(label), Some(value))
    });
    makers.chain(services).collect()
}

pub fn ble(advert: &BleAdvert) -> EventDetail {
    let head: Rows = vec![
        (
            "Address",
            advert.address.as_ref().map(|a| a.address.clone()),
        ),
        (
            "Address type",
            advert.address.as_ref().map(|a| a.kind.label().to_owned()),
        ),
        ("Target", advert.target.as_ref().map(|a| a.address.clone())),
        ("Name", advert.name.clone()),
        (
            "Beacon",
            advert.beacon.as_ref().map(|b| b.label().to_owned()),
        ),
        ("Beacon data", advert.beacon.as_ref().map(ble_beacon_text)),
    ];
    let tail: Rows = vec![
        ("URI", advert.uri.clone()),
        ("Flags", advert.flags.map(ble_flags)),
        (
            "Tx power",
            advert.tx_power_dbm.map(|dbm| format!("{dbm} dBm")),
        ),
        (
            "Appearance",
            advert.appearance.map(|a| format!("0x{a:04x}")),
        ),
        ("PDU", some(advert.pdu.label())),
        ("PHY", some(advert.phy.label())),
        ("Channel", number(advert.channel)),
        (
            "Set",
            advert
                .adi
                .map(|adi| format!("{} · data {}", adi.set, adi.data_id)),
        ),
        (
            "Aux",
            advert.aux.map(|aux| {
                format!(
                    "channel {} · {} · +{} µs",
                    aux.channel,
                    aux.phy.label(),
                    aux.offset_us
                )
            }),
        ),
        ("Level", some(format!("{:.1} dBFS", advert.level_dbfs))),
        (
            "Repeats",
            (advert.repeats > 0).then(|| advert.repeats.to_string()),
        ),
        (
            "Rejected",
            (advert.rejected > 0).then(|| advert.rejected.to_string()),
        ),
    ];
    let rows = named(head)
        .into_iter()
        .chain(ble_payload_rows(advert))
        .chain(named(tail));
    detail(rows, some(&advert.data))
}
