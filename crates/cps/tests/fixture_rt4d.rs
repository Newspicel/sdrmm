#![allow(clippy::unwrap_used, clippy::expect_used)]

use sdrmm_cps::{Image, models};
use sdrmm_wire::cps::{
    ALL_CALL_NUMBER, Admit, Bandwidth, ChannelKind, ChannelMode, Contact, ContactKind, Power,
    TimeSlot, Tone,
};

const FIXTURE: &[u8] = include_bytes!("../../../fixtures/cps/radtel-rt4d-handbuilt.img");

fn fixture() -> Image {
    Image::from_bytes(FIXTURE).expect("the RT-4D image parses")
}

#[test]
fn an_rt4d_image_laid_out_by_hand_decodes_into_the_codeplug_it_holds() {
    let model = models().get("radtel-rt4d").expect("RT4D");
    let codeplug = model.decode(&fixture()).expect("decode");
    let counts = codeplug.counts();

    assert_eq!(counts.channels, 4);
    assert_eq!(counts.zones, 2);
    assert_eq!(counts.contacts, 3);
    assert_eq!(counts.group_lists, 1);
    assert_eq!(counts.scan_lists, 0);

    assert_eq!(
        codeplug.settings.default_radio_id.as_deref(),
        Some("Radio ID")
    );
    assert_eq!(codeplug.radio_ids.len(), 1);
    assert_eq!(codeplug.radio_ids[0].number, 2_628_107);

    assert_eq!(
        codeplug.contacts,
        vec![
            Contact {
                name: "DL Berlin".to_owned(),
                kind: ContactKind::Group,
                number: 2621,
                ring: false,
            },
            Contact {
                name: "DL Nord".to_owned(),
                kind: ContactKind::Group,
                number: 2622,
                ring: false,
            },
            Contact {
                name: "All Call".to_owned(),
                kind: ContactKind::All,
                number: ALL_CALL_NUMBER,
                ring: false,
            },
        ]
    );

    assert_eq!(codeplug.group_lists[0].name, "DL Talk");
    assert_eq!(
        codeplug.group_lists[0].contacts,
        vec!["DL Berlin".to_owned(), "DL Nord".to_owned()]
    );

    let first = &codeplug.channels[0];
    assert_eq!(first.name, "DB0ABC TS1");
    assert_eq!(first.rx_hz, 439_562_500);
    assert_eq!(first.tx_hz, 431_962_500);
    assert_eq!(first.power, Power::High);
    assert!(!first.rx_only);
    let ChannelMode::Dmr(params) = &first.mode else {
        panic!("DB0ABC TS1 is a digital channel");
    };
    assert_eq!(params.color_code, 1);
    assert_eq!(params.time_slot, TimeSlot::One);
    assert_eq!(params.contact.as_deref(), Some("DL Berlin"));
    assert_eq!(params.group_list.as_deref(), Some("DL Talk"));
    assert_eq!(params.radio_id, None);
    assert_eq!(params.admit, Admit::Always);

    let ChannelMode::Dmr(params) = &codeplug.channels[1].mode else {
        panic!("DB0ABC TS2 is a digital channel");
    };
    assert_eq!(params.time_slot, TimeSlot::Two);
    assert_eq!(params.contact.as_deref(), Some("DL Nord"));
    assert_eq!(params.admit, Admit::ColorCodeFree);

    let analogue = &codeplug.channels[2];
    assert_eq!(analogue.name, "DB0ABC FM");
    assert_eq!(analogue.rx_hz, 145_600_000);
    assert_eq!(analogue.tx_hz, 145_000_000);
    assert_eq!(analogue.power, Power::Low);
    let ChannelMode::Fm(params) = &analogue.mode else {
        panic!("DB0ABC FM is an analogue channel");
    };
    assert_eq!(params.bandwidth, Bandwidth::Wide);
    assert_eq!(params.rx_tone, None);
    assert_eq!(params.tx_tone, Some(Tone::Ctcss { decihertz: 670 }));
    assert_eq!(params.admit, Admit::ChannelFree);

    let listening = &codeplug.channels[3];
    assert_eq!(listening.name, "PMR 446 1");
    assert_eq!(listening.rx_hz, 446_006_250);
    assert!(listening.rx_only);
    let ChannelMode::Fm(params) = &listening.mode else {
        panic!("PMR 446 1 is an analogue channel");
    };
    assert_eq!(params.bandwidth, Bandwidth::Narrow);
    assert_eq!(params.tx_tone, None);

    let names: Vec<_> = codeplug
        .zones
        .iter()
        .map(|zone| zone.name.clone())
        .collect();
    assert_eq!(names, vec!["70cm DMR".to_owned(), "Analog".to_owned()]);
    assert_eq!(
        codeplug.zones[0].channels_a,
        vec!["DB0ABC TS1".to_owned(), "DB0ABC TS2".to_owned()]
    );
    assert_eq!(
        codeplug.zones[1].channels_a,
        vec!["DB0ABC FM".to_owned(), "PMR 446 1".to_owned()]
    );
    assert!(codeplug.zones[0].channels_b.is_empty());
}

fn rewritten_bytes(before: &Image, after: &Image) -> Vec<String> {
    let mut changed = Vec::new();
    for (addr, held) in before.segments() {
        let now = after.get(addr, held.len()).expect("the region survives");
        for (offset, (held, now)) in held.iter().zip(now).enumerate() {
            if held != now {
                let at = addr + offset as u32;
                changed.push(format!("{at:#010x}: {held:#04x} became {now:#04x}"));
            }
        }
    }
    changed
}

#[test]
fn re_encoding_the_rt4d_image_changes_no_byte_the_radio_already_holds() {
    let model = models().get("radtel-rt4d").expect("RT4D");
    let image = fixture();
    let codeplug = model.decode(&image).expect("decode");
    let mut written = image.clone();
    let report = model.encode(&codeplug, &mut written).expect("encode");
    assert!(report.is_clean(), "{:?}", report.issues);
    let changed = rewritten_bytes(&image, &written);
    assert!(
        changed.is_empty(),
        "{} bytes would be rewritten, first {}",
        changed.len(),
        changed[0]
    );
}

#[test]
fn an_rt4d_codeplug_survives_the_trip_to_another_radio() {
    let radtel = models().get("radtel-rt4d").expect("RT4D");
    let anytone = models().get("anytone-d890uv").expect("D890UV");
    let source = radtel.decode(&fixture()).expect("decode");

    let mut target = anytone.blank_image();
    let report = anytone.encode(&source, &mut target).expect("encode");
    let landed = anytone.decode(&target).expect("decode");

    assert_eq!(report.before.channels, 4);
    assert_eq!(report.after.channels, 4);
    assert_eq!(landed.channels.len(), source.channels.len());
    assert_eq!(landed.zones.len(), source.zones.len());
    assert_eq!(landed.contacts.len(), source.contacts.len());
    assert!(
        landed
            .channels
            .iter()
            .any(|channel| channel.mode.kind() == ChannelKind::Dmr),
        "the DMR channels must survive the move"
    );
    for (landed, source) in landed.channels.iter().zip(source.channels.iter()) {
        assert_eq!(landed.name, source.name);
        assert_eq!(landed.rx_hz, source.rx_hz);
        assert_eq!(landed.tx_hz, source.tx_hz);
    }
}
