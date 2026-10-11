use sdrmm_wire::{
    lora::LoraFrame,
    remote_id::{RemoteIdFrame, RemoteIdMessage},
};

#[must_use]
pub fn lora(previous: &LoraFrame, next: &LoraFrame) -> LoraFrame {
    if next.position().is_some() || previous.position().is_none() {
        return next.clone();
    }
    LoraFrame {
        decoded: previous.decoded.clone(),
        ..next.clone()
    }
}

fn slot(message: &RemoteIdMessage) -> String {
    match message {
        RemoteIdMessage::BasicId { id_type, .. } => format!("basic_id:{id_type:?}"),
        RemoteIdMessage::Authentication { page, .. } => format!("authentication:{page}"),
        RemoteIdMessage::Unknown { message_type, .. } => format!("unknown:{message_type}"),
        RemoteIdMessage::Location(_) => "location".to_owned(),
        RemoteIdMessage::SelfId { .. } => "self_id".to_owned(),
        RemoteIdMessage::System(_) => "system".to_owned(),
        RemoteIdMessage::OperatorId { .. } => "operator_id".to_owned(),
    }
}

#[must_use]
pub fn remote_id(previous: &RemoteIdFrame, next: &RemoteIdFrame) -> RemoteIdFrame {
    let fresh: Vec<String> = next.messages.iter().map(slot).collect();
    let kept = previous
        .messages
        .iter()
        .filter(|message| !fresh.contains(&slot(message)))
        .cloned();
    RemoteIdFrame {
        uas_id: next.uas_id.clone().or_else(|| previous.uas_id.clone()),
        ssid: next.ssid.clone().or_else(|| previous.ssid.clone()),
        messages: next.messages.iter().cloned().chain(kept).collect(),
        ..next.clone()
    }
}

#[cfg(test)]
pub mod tests {
    use sdrmm_wire::{
        lora::{
            LoraCodingRate, LoraIntegrity, LoraPayload, MeshtasticContent, MeshtasticEncryption,
            MeshtasticPacket,
        },
        remote_id::{
            HeightReference, OperatorLocationType, RemoteIdLocation, RemoteIdPhy, RemoteIdSystem,
            RemoteIdTransport, UaStatus,
        },
    };

    use super::*;

    pub fn meshtastic(content: Option<MeshtasticContent>) -> LoraFrame {
        let packet = MeshtasticPacket {
            to: MeshtasticPacket::BROADCAST,
            from: 0xa1b2_c3d4,
            id: 7,
            hop_limit: 3,
            hop_start: 3,
            want_ack: false,
            via_mqtt: false,
            channel_hash: 8,
            next_hop: 0,
            relay_node: 0,
            encryption: MeshtasticEncryption::Channel,
            channel: None,
            port: None,
            port_name: None,
            request_id: None,
            reply_id: None,
            content,
        };
        LoraFrame {
            spreading_factor: 11,
            bandwidth_hz: 250_000.0,
            coding_rate: LoraCodingRate::Cr45,
            sync_word: 0x2b,
            implicit_header: false,
            low_data_rate: false,
            inverted_iq: false,
            integrity: LoraIntegrity::CrcOk,
            fec_corrected: 0,
            snr_db: 3.0,
            frequency_error_hz: 0.0,
            payload: "00".to_owned(),
            decoded: Some(LoraPayload::Meshtastic(packet)),
        }
    }

    pub fn placed(lat: f64, lon: f64) -> MeshtasticContent {
        MeshtasticContent::Position {
            lat: Some(lat),
            lon: Some(lon),
            altitude_m: None,
            time: None,
            satellites: None,
            ground_speed_kmh: None,
            precision_bits: None,
        }
    }

    pub fn location(lat: f64, lon: f64) -> RemoteIdLocation {
        RemoteIdLocation {
            status: UaStatus::Airborne,
            lat: Some(lat),
            lon: Some(lon),
            track_deg: Some(90.0),
            speed_mps: Some(4.5),
            vertical_speed_mps: None,
            pressure_altitude_m: None,
            geodetic_altitude_m: Some(510.0),
            height_m: Some(42.0),
            height_reference: HeightReference::Takeoff,
            horizontal_accuracy_m: None,
            vertical_accuracy_m: None,
            pressure_accuracy_m: None,
            speed_accuracy_mps: None,
            seconds_after_hour: None,
            timestamp_accuracy_s: None,
        }
    }

    pub fn pilot(lat: f64, lon: f64) -> RemoteIdSystem {
        RemoteIdSystem {
            operator_location_type: OperatorLocationType::Takeoff,
            operator_lat: Some(lat),
            operator_lon: Some(lon),
            operator_altitude_m: None,
            area_count: 1,
            area_radius_m: 0,
            area_ceiling_m: None,
            area_floor_m: None,
            classification: None,
            timestamp: None,
        }
    }

    pub fn drone(uas_id: Option<&str>, messages: Vec<RemoteIdMessage>) -> RemoteIdFrame {
        RemoteIdFrame {
            transport: RemoteIdTransport::BluetoothLegacy,
            phy: RemoteIdPhy::Le1m,
            address: "D2:11:22:33:44:55".to_owned(),
            channel: Some(37),
            counter: None,
            uas_id: uas_id.map(str::to_owned),
            ssid: None,
            level_dbfs: -41.0,
            messages,
            rejected: 0,
        }
    }

    #[test]
    fn a_lora_node_keeps_its_last_place_through_other_packets() {
        let there = meshtastic(Some(placed(47.5, 8.25)));
        let chatter = meshtastic(Some(MeshtasticContent::Text {
            text: "hi".to_owned(),
        }));
        assert_eq!(lora(&there, &chatter).position(), Some((47.5, 8.25)));
        let moved = meshtastic(Some(placed(47.6, 8.3)));
        assert_eq!(lora(&there, &moved).position(), Some((47.6, 8.3)));
        assert_eq!(lora(&chatter, &chatter), chatter);
    }

    #[test]
    fn a_drone_keeps_messages_the_new_frame_did_not_repeat() {
        let first = drone(
            Some("1596F"),
            vec![
                RemoteIdMessage::Location(location(47.0, 8.0)),
                RemoteIdMessage::System(pilot(47.001, 8.001)),
            ],
        );
        let second = drone(None, vec![RemoteIdMessage::Location(location(47.1, 8.1))]);
        let merged = remote_id(&first, &second);
        assert_eq!(merged.uas_id.as_deref(), Some("1596F"));
        assert_eq!(merged.messages.len(), 2);
        assert_eq!(merged.position(), Some((47.1, 8.1)));
        assert!(merged.system().is_some());
    }
}
