use super::*;

fn readout_keys(face: &ChannelStatus) -> Vec<&str> {
    face.readouts
        .iter()
        .map(|field| field.key.as_str())
        .collect()
}

#[test]
fn a_published_status_is_an_event_and_offers_its_services() {
    let status = BroadcastStatus {
        system: "dab".to_owned(),
        locked: true,
        services: vec![
            BroadcastService {
                id: 0x1234,
                label: "Radio One".to_owned(),
                ..BroadcastService::default()
            },
            BroadcastService {
                id: 7,
                label: " ".to_owned(),
                ..BroadcastService::default()
            },
        ],
        ..BroadcastStatus::default()
    };
    let mut out = ChannelOutputs::default();

    publish_status(&mut out, &status, "service_id");

    assert_eq!(latest_status(&out), Some(status));
    let face = out.status.expect("a channel status");
    assert_eq!(face.health.lock, Some(true));
    assert_eq!(readout_keys(&face), ["system"]);
    let [choice] = face.choices.as_slice() else {
        panic!("one choice: {:?}", face.choices);
    };
    assert_eq!(choice.name, "service_id");
    let options: Vec<_> = choice
        .options
        .iter()
        .map(|option| (option.value.clone(), option.label.as_str()))
        .collect();
    assert_eq!(
        options,
        [
            (Value::Null, "Auto"),
            (0x1234.into(), "Radio One"),
            (7.into(), "7")
        ]
    );
    assert_eq!(choice.options[0].status.as_deref(), Some(SERVICE_READOUT));
}

#[test]
fn the_health_footer_shows_snr_and_offset_only_while_locked() {
    let locked = BroadcastStatus {
        system: "dab".to_owned(),
        system_label: "DAB".to_owned(),
        locked: true,
        snr_db: 14.5,
        frequency_error_hz: -120.0,
        transmission_mode: Some("ii".to_owned()),
        transmission_mode_label: Some("II".to_owned()),
        ..BroadcastStatus::default()
    };
    let mut out = ChannelOutputs::default();
    publish_status(&mut out, &locked, "service_id");
    let face = out.status.take().expect("a channel status");
    assert_eq!(
        face.health,
        ChannelHealth::default()
            .lock(true)
            .snr(14.5)
            .freq_error(-120.0)
    );
    assert_eq!(
        face.readout(TRANSMISSION_MODE_READOUT)
            .map(|field| &field.value),
        Some(&FieldValue::Text("II".to_owned()))
    );
    assert_eq!(
        face.readout(SYSTEM_READOUT).map(|field| &field.value),
        Some(&FieldValue::Text("DAB".to_owned()))
    );

    publish_status(
        &mut out,
        &BroadcastStatus {
            locked: false,
            ..locked
        },
        "service_id",
    );
    let face = out.status.expect("a channel status");
    assert_eq!(face.health, ChannelHealth::default().lock(false));
}

#[test]
fn a_face_shows_how_the_stream_decodes_not_what_it_decoded() {
    let status = BroadcastStatus {
        system: "dab_plus".to_owned(),
        locked: true,
        label: Some("Radio One".to_owned()),
        ensemble_label: Some("Ensemble".to_owned()),
        bitrate_kbps: Some(64),
        dynamic_label: Some("Song".to_owned()),
        frames_ok: 10,
        ..BroadcastStatus::default()
    };
    let mut out = ChannelOutputs::default();

    publish_status(&mut out, &status, "service_id");
    let face = out.status.take().expect("a channel status");
    assert_eq!(
        readout_keys(&face),
        [SYSTEM_READOUT, SERVICE_READOUT, "bitrate"]
    );

    publish_status(
        &mut out,
        &BroadcastStatus {
            frames_bad: 2,
            ..status
        },
        "service_id",
    );
    let face = out.status.expect("a channel status");
    assert_eq!(
        readout_keys(&face),
        [
            SYSTEM_READOUT,
            SERVICE_READOUT,
            "bitrate",
            BAD_FRAMES_READOUT
        ]
    );
}

#[test]
fn a_face_names_why_no_audio_plays() {
    let silent = BroadcastStatus {
        system: "dab_plus".to_owned(),
        audio_error: Some("Decoder unavailable: FDK AAC".to_owned()),
        ..BroadcastStatus::default()
    };
    let mut out = ChannelOutputs::default();

    publish_status(&mut out, &silent, "service_id");
    let face = out.status.take().expect("a channel status");
    let audio = face
        .readouts
        .iter()
        .find(|field| field.key == AUDIO_READOUT)
        .expect("an audio readout");
    assert_eq!(
        audio.value,
        FieldValue::Text("Decoder unavailable: FDK AAC".to_owned())
    );

    publish_status(
        &mut out,
        &BroadcastStatus {
            audio_frames_ok: 1,
            ..silent
        },
        "service_id",
    );
    let face = out.status.expect("a channel status");
    assert!(!readout_keys(&face).contains(&AUDIO_READOUT));
}

#[test]
fn no_services_found_offers_no_auto_either() {
    let mut out = ChannelOutputs::default();
    publish_status(&mut out, &BroadcastStatus::default(), "service_id");
    let face = out.status.expect("a channel status");
    assert!(face.choices[0].options.is_empty());
}

#[test]
fn a_decoder_reading_travels_under_its_own_key() {
    let mut status = BroadcastStatus {
        system: "dvb_s2".to_owned(),
        ..BroadcastStatus::default()
    };
    status
        .extra
        .insert("superframe".to_owned(), serde_json::json!({"format": 2}));
    let json = serde_json::to_value(&status).expect("status serializes");
    assert_eq!(json["superframe"]["format"], 2);
    assert_eq!(
        serde_json::from_value::<BroadcastStatus>(json).expect("status reads back"),
        status
    );
}

#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum Generation {
    DabPlus,
}

impl SettingChoice for Generation {
    fn all() -> &'static [Self] {
        &[Self::DabPlus]
    }

    fn choice_label(self) -> &'static str {
        "DAB+"
    }
}

#[test]
fn the_event_carries_the_choice_id_and_the_face_its_label() {
    let status = BroadcastStatus::default()
        .with_system(Generation::DabPlus)
        .with_transmission_mode(Some(Generation::DabPlus));
    let json = serde_json::to_value(&status).expect("status serializes");
    assert_eq!(json["system"], "dab_plus");
    assert_eq!(json["transmission_mode"], "dab_plus");
    let mut out = ChannelOutputs::default();
    publish_status(&mut out, &status, "service_id");
    let face = out.status.expect("a channel status");
    for key in [SYSTEM_READOUT, TRANSMISSION_MODE_READOUT] {
        assert_eq!(
            face.readout(key).map(|field| &field.value),
            Some(&FieldValue::Text("DAB+".to_owned()))
        );
    }
}

#[test]
fn a_status_without_labels_shows_its_ids() {
    let status = BroadcastStatus {
        system: "dab".to_owned(),
        ..BroadcastStatus::default()
    };
    let presented = status.present(&BroadcastStatus::KIND);
    assert_eq!(
        presented
            .fields
            .iter()
            .find(|field| field.key == SYSTEM_READOUT)
            .map(|field| &field.value),
        Some(&FieldValue::Text("dab".to_owned()))
    );
}
