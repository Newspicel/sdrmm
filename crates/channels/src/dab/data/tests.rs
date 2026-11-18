use std::time::{Duration, Instant};

use sdrmm_wire::BroadcastData;

use super::*;
use crate::dab::packet::tests::packet;

const CONFIG: PacketConfig = PacketConfig {
    address: 17,
    kind: 5,
    data_groups: true,
    fec: false,
};

fn settle(data: &mut ServiceData, out: &mut ChannelOutputs) {
    let until = Instant::now() + Duration::from_secs(2);
    while !data.input_idle() && Instant::now() < until {
        thread::sleep(Duration::from_millis(1));
    }
    thread::sleep(Duration::from_millis(20));
    data.drain(out);
}

fn send_object(data: &mut ServiceData) {
    data.packets(CONFIG, &packet(17, 0, 2, b"\0\0first "));
    data.packets(CONFIG, &packet(17, 1, 1, b"last"));
}

#[test]
fn objects_read_off_the_dsp_thread_carry_the_service() {
    let mut data = ServiceData::new().expect("worker");
    let mut out = ChannelOutputs::default();
    data.select(0x5001, None);
    send_object(&mut data);
    settle(&mut data, &mut out);
    let objects: Vec<_> = out
        .events
        .iter()
        .filter_map(sdrmm_wire::DecoderEvent::read::<BroadcastData>)
        .collect();
    assert_eq!(objects.len(), 1, "{:?}", data.error);
    assert_eq!(objects[0].bytes, b"first last");
    assert_eq!(objects[0].service_id, Some(0x5001));
    assert_eq!(data.groups, 1);
}

#[test]
fn a_reset_drops_what_the_old_service_left_queued() {
    let mut data = ServiceData::new().expect("worker");
    let mut out = ChannelOutputs::default();
    send_object(&mut data);
    data.reset();
    settle(&mut data, &mut out);
    assert!(out.events.is_empty());
    assert_eq!(data.groups, 0);
}

#[test]
fn a_unit_past_one_chunk_is_counted_not_dropped_silently() {
    let mut data = ServiceData::new().expect("worker");
    data.access_unit(&[0; CHUNK_BYTES + 1]);
    assert_eq!(data.errors, 1);
    assert_eq!(data.error.as_deref(), Some("DAB data unit too long"));
}
