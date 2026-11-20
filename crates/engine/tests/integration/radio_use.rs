#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use sdrmm_channels::NfmParams;
use sdrmm_decoder::encoder::EncoderParams;
use sdrmm_device::DeviceRegistry;
use sdrmm_device_virtual::VirtualDriver;
use sdrmm_engine::{Engine, EngineError, SET_TO_RECEIVE, TransmitRequest, TransmitSource};
use sdrmm_wire::{
    ChannelParams, ChannelSettings, DeviceSet, RadioActivity, RadioUse, Squelch, TransmitSettings,
};

const CENTER_HZ: f64 = 100_000_000.0;
const HALF_DUPLEX: &str = "virtual:halfduplex";

fn engine() -> Arc<Engine> {
    let mut registry = DeviceRegistry::new();
    registry.register(10, Box::new(VirtualDriver::new()));
    Engine::with_registry(registry, None)
}

fn set(engine: &Engine, ds: u32) -> DeviceSet {
    engine
        .snapshot()
        .device_sets
        .into_iter()
        .find(|set| set.id == ds)
        .expect("open radio")
}

fn nfm_request() -> TransmitRequest {
    TransmitRequest {
        node: "tx".to_owned(),
        source: TransmitSource::Encoder(EncoderParams::bare("nfm")),
        settings: TransmitSettings::default(),
        audio: Default::default(),
    }
}

fn listener(engine: &Engine, ds: u32) -> u32 {
    let settings = ChannelSettings {
        frequency_hz: CENTER_HZ,
        squelch: Squelch::Off,
        params: ChannelParams::new("nfm", &NfmParams::default()).expect("nfm"),
        blanker: Default::default(),
        if_filter: Default::default(),
    };
    engine.add_channel(ds, 0, settings).expect("channel")
}

/// Whether IQ reaches the channel within a short wait.
fn hears(engine: &Engine, ds: u32, channel: u32) -> bool {
    let mut iq = engine.subscribe_iq(ds, channel).expect("iq");
    let deadline = Instant::now() + Duration::from_millis(800);
    while Instant::now() < deadline {
        if iq.try_recv().is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn a_radio_set_to_receive_refuses_to_key() {
    let engine = engine();
    let ds = engine
        .open_device_set(HALF_DUPLEX, RadioUse::Receive)
        .expect("radio");
    let open = set(&engine, ds);
    assert_eq!(open.radio_use, RadioUse::Receive);
    assert_eq!(
        open.uses,
        [RadioUse::Receive, RadioUse::Send, RadioUse::Both]
    );
    assert_eq!(open.activity, RadioActivity::Receiving);
    match engine.arm_transmitter(ds, &nfm_request()) {
        Err(EngineError::Transmit(reason)) => assert_eq!(reason, SET_TO_RECEIVE),
        other => panic!("must be refused, got {other:?}"),
    }

    engine.set_radio_use(ds, RadioUse::Both).expect("both");
    engine.arm_transmitter(ds, &nfm_request()).expect("armed");
    engine
        .set_radio_use(ds, RadioUse::Receive)
        .expect("receive");
    assert!(
        set(&engine, ds).transmit.is_none(),
        "receive drops the transmitter"
    );
}

#[test]
fn a_radio_set_to_send_runs_no_receiver_until_it_is_told_to() {
    let engine = engine();
    let ds = engine
        .open_device_set(HALF_DUPLEX, RadioUse::Send)
        .expect("radio");
    assert_eq!(set(&engine, ds).activity, RadioActivity::Idle);
    let channel = listener(&engine, ds);
    assert!(!hears(&engine, ds, channel), "a send-only radio is deaf");

    engine.set_radio_use(ds, RadioUse::Both).expect("both");
    assert_eq!(set(&engine, ds).activity, RadioActivity::Receiving);
    assert!(hears(&engine, ds, channel), "receiving starts in place");

    engine.set_radio_use(ds, RadioUse::Send).expect("send");
    assert_eq!(set(&engine, ds).activity, RadioActivity::Idle);
    assert!(!hears(&engine, ds, channel), "and stops again");
}

#[test]
fn a_receive_only_radio_stays_receive_only() {
    let engine = engine();
    let ds = engine
        .open_device_set("virtual:quad", RadioUse::Both)
        .expect("radio");
    assert_eq!(set(&engine, ds).radio_use, RadioUse::Receive);
    assert_eq!(set(&engine, ds).uses, [RadioUse::Receive]);
}
