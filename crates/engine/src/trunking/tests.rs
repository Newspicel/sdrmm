use sdrmm_channels::{DmrParams, DmrSlots, P25Params};
use sdrmm_device::DeviceRegistry;
use sdrmm_device_virtual::VirtualDriver;
use sdrmm_wire::{
    ChannelParams, ChannelSettings, DecoderEvent, DeviceSettings, DvChannelDefinition,
    TrunkChannelSource, TrunkControl,
};

use super::{control::next_control, *};

const LOGICAL_CHANNEL: u16 = 17;

fn engine() -> Arc<Engine> {
    let mut registry = DeviceRegistry::new();
    registry.register(1, Box::new(VirtualDriver::new()));
    Engine::with_registry(registry, None)
}

fn control_channel(engine: &Engine) -> (u32, u32, f64) {
    control_channel_of(
        engine,
        ChannelParams::new("dmr", &DmrParams::default()).expect("dmr settings"),
    )
}

fn control_channel_of(engine: &Engine, params: ChannelParams) -> (u32, u32, f64) {
    let device_set = engine
        .create_device_set("virtual:band")
        .expect("virtual device");
    let center_hz = engine.snapshot().device_sets[0]
        .settings
        .center_hz
        .expect("a tuned radio");
    let channel = engine
        .add_channel(
            device_set,
            0,
            ChannelSettings {
                frequency_hz: center_hz,
                squelch: sdrmm_wire::Squelch::Off,
                params,
                blanker: Default::default(),
                if_filter: Default::default(),
            },
        )
        .expect("control channel");
    (device_set, channel, center_hz)
}

fn follower(engine: &Arc<Engine>, protocol: &str, carrier: (u32, u32)) -> Follower {
    searching_follower(
        engine,
        protocol,
        carrier,
        TrunkDiscovery::default(),
        Vec::new(),
    )
}

fn searching_follower(
    engine: &Arc<Engine>,
    protocol: &str,
    carrier: (u32, u32),
    discovery: TrunkDiscovery,
    channel_map: Vec<TrunkChannelEntry>,
) -> Follower {
    let control_hz = engine
        .snapshot()
        .device_sets
        .iter()
        .find(|set| set.id == carrier.0)
        .and_then(|set| set.settings.center_hz)
        .expect("a tuned radio") as u64;
    let mut follower = Follower {
        engine: Arc::downgrade(engine),
        status: Arc::new(Mutex::new(Vec::new())),
        dropped: Arc::new(AtomicU64::new(0)),
        sites: HashMap::new(),
        systems: vec![TrunkSystem {
            node: "trunk".to_owned(),
            decoder: String::new(),
            protocol: protocol.to_owned(),
            discovery,
            channel_map,
            learned: Vec::new(),
            talkgroups: TrunkTalkgroups::default(),
            keys: Vec::new(),
            vocoder: None,
            radio: Some(TrunkRadio {
                device_set: carrier.0,
                stream: 0,
                control_hz,
                ignore_crc: false,
            }),
        }],
        controls: HashMap::from([(
            "trunk".to_owned(),
            ControlChannel {
                device_set: carrier.0,
                channel: carrier.1,
                freq_hz: control_hz,
                configured_hz: control_hz,
                heard: Instant::now(),
            },
        )]),
        carriers: HashMap::new(),
        definitions: HashMap::new(),
        followers: HashMap::new(),
        problems: HashMap::new(),
        detected: HashMap::new(),
        prospectors: HashMap::new(),
        probes: HashMap::new(),
        unreachable: HashSet::new(),
        watching: Vec::new(),
        tx: mpsc::sync_channel(1).0,
    };
    follower.reconcile();
    follower
}

fn record(carrier: (u32, u32), frame: DvFrame) -> DecodedRecord {
    DecodedRecord {
        origin: None,
        sinks: Vec::new(),
        callsign: None,
        device_set: carrier.0,
        channel: carrier.1,
        at: "2026-08-14T10:00:00Z".to_owned(),
        freq_hz: 451_000_000.0,
        event: DecoderEvent::of(&frame),
        view: None,
    }
}

fn definition(rx_hz: u64) -> DvFrame {
    DvFrame {
        trunk_protocol: Some("tier_three".to_owned()),
        crc_verified: Some(true),
        channel_definition: Some(DvChannelDefinition {
            channel: LOGICAL_CHANNEL,
            tx_hz: rx_hz,
            rx_hz,
            color_code: None,
        }),
        ..DvFrame::new("DMR", DvFrameKind::Control)
    }
}

fn grant(slot: u8) -> DvFrame {
    DvFrame {
        trunk_protocol: Some("tier_three".to_owned()),
        crc_verified: Some(true),
        channel: Some(LOGICAL_CHANNEL),
        slot: Some(slot),
        destination: Some(91),
        ..DvFrame::new("DMR", DvFrameKind::Control)
    }
}

fn search_over(center_hz: f64) -> TrunkDiscovery {
    TrunkDiscovery {
        enabled: true,
        ranges: vec![sdrmm_wire::TrunkSearchRange {
            start_hz: center_hz as u64 + 12_500,
            end_hz: center_hz as u64 + 50_000,
            step_hz: 12_500,
        }],
        max_probes: 4,
    }
}

fn traffic(destination: u32, source: u32, slot: u8) -> DvFrame {
    DvFrame {
        slot: Some(slot),
        destination: Some(destination),
        source: Some(source),
        ..DvFrame::new("DMR", DvFrameKind::Header)
    }
}

fn identified_grant(slot: u8) -> DvFrame {
    DvFrame {
        source: Some(4001),
        ..grant(slot)
    }
}

fn status(follower: &Follower) -> TrunkSystemStatus {
    follower
        .status
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .first()
        .cloned()
        .expect("one system")
}

#[test]
fn a_grant_on_a_defined_channel_opens_a_slot_filtered_receiver() {
    let engine = engine();
    let (device_set, channel, center_hz) = control_channel(&engine);
    let carrier = (device_set, channel);
    let mut follower = follower(&engine, "tier_three", carrier);
    let traffic = center_hz as u64 + 125_000;

    follower.observe(&record(carrier, definition(traffic)));
    follower.observe(&record(carrier, grant(2)));

    let set = engine.snapshot().device_sets.remove(0);
    assert_eq!(set.channels.len(), 2, "the follower was not created");
    let followed = &set.channels[1];
    assert_eq!(followed.node.as_deref(), Some("trunk"));
    assert_eq!(followed.settings.frequency_hz, traffic as f64);
    assert_eq!(
        followed.settings.params,
        ChannelParams::new(
            "dmr",
            &DmrParams {
                slots: DmrSlots::Two,
                ignore_crc: false,
                ..DmrParams::default()
            }
        )
        .expect("dmr settings")
    );
    let status = status(&follower);
    assert_eq!(status.followers.len(), 1);
    assert_eq!(status.followers[0].freq_hz, traffic);
    assert_eq!(status.followers[0].logical_channel, Some(LOGICAL_CHANNEL));
    assert!(status.problems.is_empty());
}

#[test]
fn a_followed_call_is_deciphered_with_the_systems_keys() {
    let engine = engine();
    let (device_set, channel, center_hz) = control_channel(&engine);
    let carrier = (device_set, channel);
    let mut follower = follower(&engine, "tier_three", carrier);
    let keys = vec![VoiceKey {
        key_id: 137,
        key: "58FFA35A8C".to_owned(),
    }];
    follower.systems[0].keys = keys.clone();
    follower.reconcile();
    let traffic = center_hz as u64 + 125_000;

    follower.observe(&record(carrier, definition(traffic)));
    follower.observe(&record(carrier, grant(2)));

    let set = engine.snapshot().device_sets.remove(0);
    let params = set.channels[1]
        .settings
        .params
        .parse::<DmrParams>()
        .expect("dmr settings");
    assert_eq!(params.keys, keys);
}

#[test]
fn a_key_the_decoder_refuses_is_reported_as_a_problem() {
    let engine = engine();
    let (device_set, channel, center_hz) = control_channel(&engine);
    let carrier = (device_set, channel);
    let mut follower = follower(&engine, "tier_three", carrier);
    follower.systems[0].keys = vec![VoiceKey {
        key_id: 1,
        key: "123456".to_owned(),
    }];
    follower.reconcile();
    let traffic = center_hz as u64 + 125_000;

    follower.observe(&record(carrier, definition(traffic)));
    follower.observe(&record(carrier, grant(2)));

    assert_eq!(engine.snapshot().device_sets[0].channels.len(), 1);
    assert_eq!(status(&follower).problems.len(), 1);
}

#[test]
fn a_grant_for_an_undefined_channel_opens_nothing() {
    let engine = engine();
    let (device_set, channel, _) = control_channel(&engine);
    let carrier = (device_set, channel);
    let mut follower = follower(&engine, "tier_three", carrier);

    follower.observe(&record(carrier, grant(1)));

    assert_eq!(engine.snapshot().device_sets[0].channels.len(), 1);
    assert!(follower.followers.is_empty());
}

#[test]
fn a_site_that_checksums_its_own_way_is_still_followed() {
    let engine = engine();
    let (device_set, channel, center_hz) = control_channel(&engine);
    let carrier = (device_set, channel);
    let mut follower = follower(&engine, "tier_three", carrier);
    let traffic = center_hz as u64 + 125_000;

    follower.observe(&record(carrier, definition(traffic)));
    let mut unverified = grant(1);
    unverified.crc_verified = Some(false);
    follower.observe(&record(carrier, unverified));

    let status = status(&follower);
    assert_eq!(status.followers.len(), 1, "the grant was thrown away");
    assert_eq!(status.followers[0].freq_hz, traffic);
}

#[test]
fn auto_follows_capacity_plus_once_its_signalling_identifies_it() {
    let engine = engine();
    let (device_set, channel, _) = control_channel(&engine);
    let carrier = (device_set, channel);
    let mut follower = follower(&engine, AUTO_TRUNK_PROTOCOL, carrier);
    assert!(
        follower.followers.is_empty(),
        "auto followed before it knew"
    );

    follower.observe(&record(
        carrier,
        DvFrame {
            trunk_protocol: Some("capacity_plus".to_owned()),
            crc_verified: Some(true),
            ..DvFrame::new("DMR", DvFrameKind::Control)
        },
    ));

    let set = engine.snapshot().device_sets.remove(0);
    assert_eq!(set.channels.len(), 3, "both slots were not provisioned");
    let status = status(&follower);
    assert_eq!(status.detected, Some("capacity_plus".to_owned()));
    assert_eq!(status.followers.len(), 2);
    assert_eq!(status.followers[0].slot, Some(1));
    assert_eq!(status.followers[1].slot, Some(2));
}

#[test]
fn capacity_plus_keeps_both_slots_of_every_repeater_the_plan_names() {
    let engine = engine();
    let (device_set, channel, center_hz) = control_channel(&engine);
    let carrier = (device_set, channel);
    let second = center_hz as u64 + 25_000;
    let mut follower = searching_follower(
        &engine,
        "capacity_plus",
        carrier,
        TrunkDiscovery::default(),
        vec![TrunkChannelEntry {
            lcn: 2,
            freq_hz: second,
        }],
    );

    follower.observe(&record(
        carrier,
        DvFrame {
            trunk_protocol: Some("capacity_plus".to_owned()),
            crc_verified: Some(true),
            ..DvFrame::new("DMR", DvFrameKind::Control)
        },
    ));

    let planned = status(&follower);
    assert_eq!(planned.followers.len(), 4);
    assert!(
        planned
            .followers
            .iter()
            .filter(|follower| follower.freq_hz == second)
            .count()
            == 2,
        "the second repeater of the plan was never followed"
    );

    follower.systems[0].channel_map.clear();
    follower.reconcile();

    let after = status(&follower);
    assert_eq!(
        after.followers.len(),
        2,
        "a forgotten repeater kept running"
    );
    assert!(after.followers.iter().all(|kept| kept.freq_hz != second));
}

#[test]
fn auto_follows_hytera_xpt_once_its_signalling_identifies_it() {
    let engine = engine();
    let (device_set, channel, _) = control_channel(&engine);
    let carrier = (device_set, channel);
    let mut follower = follower(&engine, AUTO_TRUNK_PROTOCOL, carrier);
    assert!(
        follower.followers.is_empty(),
        "auto followed before it knew"
    );

    follower.observe(&record(
        carrier,
        DvFrame {
            trunk_protocol: Some("hytera_xpt".to_owned()),
            crc_verified: Some(true),
            ..DvFrame::new("DMR", DvFrameKind::Control)
        },
    ));

    let set = engine.snapshot().device_sets.remove(0);
    assert_eq!(set.channels.len(), 3, "both slots were not provisioned");
    assert!(set.channels.iter().any(|channel| {
        channel.settings.params.type_id == "dmr"
            && channel
                .settings
                .params
                .parse::<DmrParams>()
                .is_ok_and(|params| params.slots == DmrSlots::One)
    }));
    assert!(set.channels.iter().any(|channel| {
        channel.settings.params.type_id == "dmr"
            && channel
                .settings
                .params
                .parse::<DmrParams>()
                .is_ok_and(|params| params.slots == DmrSlots::Two)
    }));
    let status = status(&follower);
    assert_eq!(status.detected, Some("hytera_xpt".to_owned()));
    assert_eq!(status.followers.len(), 2);
    assert_eq!(status.followers[0].slot, Some(1));
    assert_eq!(status.followers[1].slot, Some(2));
}

#[test]
fn a_grant_outside_the_sampled_band_is_reported_once_and_backed_off() {
    let engine = engine();
    let (device_set, channel, center_hz) = control_channel(&engine);
    let carrier = (device_set, channel);
    let mut follower = follower(&engine, "tier_three", carrier);
    let far = center_hz as u64 + 10_000_000;

    follower.observe(&record(carrier, definition(far)));
    follower.observe(&record(carrier, grant(1)));
    follower.observe(&record(carrier, grant(1)));

    assert_eq!(engine.snapshot().device_sets[0].channels.len(), 1);
    let status = status(&follower);
    assert_eq!(status.problems.len(), 1);
    assert_eq!(status.problems[0].freq_hz, far);
    assert_eq!(status.problems[0].attempts, 1, "the refusal was retried");
    assert!(!status.problems[0].reason.is_empty());
}

#[test]
fn dropping_the_system_closes_the_receivers_it_opened() {
    let engine = engine();
    let (device_set, channel, center_hz) = control_channel(&engine);
    let carrier = (device_set, channel);
    let mut follower = follower(&engine, "tier_three", carrier);
    follower.observe(&record(carrier, definition(center_hz as u64 + 125_000)));
    follower.observe(&record(carrier, grant(1)));
    assert_eq!(engine.snapshot().device_sets[0].channels.len(), 2);

    follower.systems.clear();
    follower.reconcile();

    assert_eq!(
        engine.snapshot().device_sets[0].channels.len(),
        0,
        "the orphaned follower was left running"
    );
    assert!(follower.followers.is_empty());
    assert!(
        follower
            .status
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty()
    );
}

#[test]
fn a_system_given_a_radio_opens_its_own_control_channel() {
    let engine = engine();
    let device_set = engine
        .create_device_set("virtual:band")
        .expect("virtual device");
    let center_hz = engine.snapshot().device_sets[0]
        .settings
        .center_hz
        .expect("a tuned radio");
    let control_hz = center_hz as u64 + 12_500;
    let mut follower = Follower {
        engine: Arc::downgrade(&engine),
        status: Arc::new(Mutex::new(Vec::new())),
        dropped: Arc::new(AtomicU64::new(0)),
        sites: HashMap::new(),
        systems: vec![TrunkSystem {
            node: "trunk".to_owned(),
            decoder: String::new(),
            protocol: "tier_three".to_owned(),
            discovery: TrunkDiscovery::default(),
            channel_map: Vec::new(),
            learned: Vec::new(),
            talkgroups: TrunkTalkgroups::default(),
            keys: Vec::new(),
            vocoder: None,
            radio: Some(TrunkRadio {
                device_set,
                stream: 0,
                control_hz,
                ignore_crc: false,
            }),
        }],
        carriers: HashMap::new(),
        definitions: HashMap::new(),
        followers: HashMap::new(),
        problems: HashMap::new(),
        detected: HashMap::new(),
        prospectors: HashMap::new(),
        probes: HashMap::new(),
        controls: HashMap::new(),
        unreachable: HashSet::new(),
        watching: Vec::new(),
        tx: mpsc::sync_channel(1).0,
    };

    follower.reconcile();

    let set = engine.snapshot().device_sets.remove(0);
    assert_eq!(set.channels.len(), 1, "no control receiver was opened");
    assert_eq!(set.channels[0].settings.frequency_hz, control_hz as f64);
    assert_eq!(status(&follower).carriers, 1);
    assert_eq!(
        status(&follower).control,
        Some(TrunkControl {
            device_set,
            channel: set.channels[0].id,
            freq_hz: control_hz,
        }),
        "the status did not name the control channel"
    );

    follower.systems.clear();
    follower.reconcile();

    assert!(
        engine.snapshot().device_sets[0].channels.is_empty(),
        "the control receiver outlived the system"
    );
}

#[test]
fn moving_the_control_frequency_retunes_the_receiver_in_place() {
    let engine = engine();
    let device_set = engine
        .create_device_set("virtual:band")
        .expect("virtual device");
    let center_hz = engine.snapshot().device_sets[0]
        .settings
        .center_hz
        .expect("a tuned radio");
    let radio = |control_hz| {
        Some(TrunkRadio {
            device_set,
            stream: 0,
            control_hz,
            ignore_crc: false,
        })
    };
    let mut follower = Follower {
        engine: Arc::downgrade(&engine),
        status: Arc::new(Mutex::new(Vec::new())),
        dropped: Arc::new(AtomicU64::new(0)),
        sites: HashMap::new(),
        systems: vec![TrunkSystem {
            node: "trunk".to_owned(),
            decoder: String::new(),
            protocol: "tier_three".to_owned(),
            discovery: TrunkDiscovery::default(),
            channel_map: Vec::new(),
            learned: Vec::new(),
            talkgroups: TrunkTalkgroups::default(),
            keys: Vec::new(),
            vocoder: None,
            radio: radio(center_hz as u64 + 12_500),
        }],
        carriers: HashMap::new(),
        definitions: HashMap::new(),
        followers: HashMap::new(),
        problems: HashMap::new(),
        detected: HashMap::new(),
        prospectors: HashMap::new(),
        probes: HashMap::new(),
        controls: HashMap::new(),
        unreachable: HashSet::new(),
        watching: Vec::new(),
        tx: mpsc::sync_channel(1).0,
    };
    follower.reconcile();
    let opened = engine.snapshot().device_sets[0].channels[0].id;

    follower.systems[0].radio = radio(center_hz as u64 + 25_000);
    follower.reconcile();

    let set = engine.snapshot().device_sets.remove(0);
    assert_eq!(set.channels.len(), 1, "a second control receiver appeared");
    assert_eq!(set.channels[0].id, opened, "the receiver was rebuilt");
    assert_eq!(set.channels[0].settings.frequency_hz, center_hz + 25_000.0);
}

#[test]
fn retuning_the_radio_keeps_the_control_receiver_on_its_frequency() {
    let engine = engine();
    let device_set = engine
        .create_device_set("virtual:band")
        .expect("virtual device");
    let center_hz = engine.snapshot().device_sets[0]
        .settings
        .center_hz
        .expect("a tuned radio");
    let control_hz = center_hz as u64 + 12_500;
    let mut follower = Follower {
        engine: Arc::downgrade(&engine),
        status: Arc::new(Mutex::new(Vec::new())),
        dropped: Arc::new(AtomicU64::new(0)),
        sites: HashMap::new(),
        systems: vec![TrunkSystem {
            node: "trunk".to_owned(),
            decoder: String::new(),
            protocol: "tier_three".to_owned(),
            discovery: TrunkDiscovery::default(),
            channel_map: Vec::new(),
            learned: Vec::new(),
            talkgroups: TrunkTalkgroups::default(),
            keys: Vec::new(),
            vocoder: None,
            radio: Some(TrunkRadio {
                device_set,
                stream: 0,
                control_hz,
                ignore_crc: false,
            }),
        }],
        carriers: HashMap::new(),
        definitions: HashMap::new(),
        followers: HashMap::new(),
        problems: HashMap::new(),
        detected: HashMap::new(),
        prospectors: HashMap::new(),
        probes: HashMap::new(),
        controls: HashMap::new(),
        unreachable: HashSet::new(),
        watching: Vec::new(),
        tx: mpsc::sync_channel(1).0,
    };
    follower.reconcile();
    let opened = engine.snapshot().device_sets[0].channels[0].id;

    engine
        .patch_device(
            device_set,
            DeviceSettings {
                center_hz: Some(center_hz + 100_000.0),
                ..DeviceSettings::default()
            },
        )
        .expect("retune the radio");
    follower.reconcile();

    let set = engine.snapshot().device_sets.remove(0);
    assert_eq!(set.channels.len(), 1, "a second control receiver appeared");
    assert_eq!(set.channels[0].id, opened, "the receiver was rebuilt");
    assert_eq!(
        set.channels[0].settings.frequency_hz, control_hz as f64,
        "the control receiver drifted with the radio"
    );
}

#[test]
fn a_data_call_is_followed_like_any_other() {
    let engine = engine();
    let (device_set, channel, center_hz) = control_channel(&engine);
    let carrier = (device_set, channel);
    let traffic_hz = center_hz as u64 + 25_000;
    let mut follower = searching_follower(
        &engine,
        "tier_three",
        carrier,
        TrunkDiscovery::default(),
        vec![TrunkChannelEntry {
            lcn: LOGICAL_CHANNEL,
            freq_hz: traffic_hz,
        }],
    );

    follower.observe(&record(
        carrier,
        DvFrame {
            trunk_protocol: Some("tier_three".to_owned()),
            crc_verified: Some(true),
            channel: Some(LOGICAL_CHANNEL),
            slot: Some(1),
            destination: Some(9_999),
            source: Some(9_998),
            data: Some("data call".to_owned()),
            ..DvFrame::new("DMR", DvFrameKind::Control)
        },
    ));

    let status = status(&follower);
    assert_eq!(status.followers.len(), 1, "a data call went unfollowed");
    assert_eq!(status.followers[0].freq_hz, traffic_hz);
}

#[test]
fn a_capacity_max_channel_update_follows_every_busy_timeslot() {
    let engine = engine();
    let (device_set, channel, center_hz) = control_channel(&engine);
    let carrier = (device_set, channel);
    let traffic_hz = center_hz as u64 + 25_000;
    let mut follower = searching_follower(
        &engine,
        "tier_three",
        carrier,
        TrunkDiscovery::default(),
        vec![TrunkChannelEntry {
            lcn: LOGICAL_CHANNEL,
            freq_hz: traffic_hz,
        }],
    );

    follower.observe(&record(
        carrier,
        DvFrame {
            trunk_protocol: Some("tier_three".to_owned()),
            crc_verified: Some(true),
            channel: Some(LOGICAL_CHANNEL),
            slot_activity: vec![
                sdrmm_wire::DvSlotActivity {
                    slot: 1,
                    activity: "group voice".to_owned(),
                    destination_hash: None,
                    destination: Some(9_001),
                    logical_channel: Some(LOGICAL_CHANNEL),
                },
                sdrmm_wire::DvSlotActivity {
                    slot: 2,
                    activity: "group voice".to_owned(),
                    destination_hash: None,
                    destination: Some(9_002),
                    logical_channel: Some(LOGICAL_CHANNEL),
                },
            ],
            ..DvFrame::new("DMR", DvFrameKind::Control)
        },
    ));

    let status = status(&follower);
    assert_eq!(status.followers.len(), 2, "one timeslot went unfollowed");
    assert!(
        status
            .followers
            .iter()
            .all(|open| open.freq_hz == traffic_hz)
    );
    assert_eq!(status.followers[0].slot, Some(1));
    assert_eq!(status.followers[1].slot, Some(2));
}

#[test]
fn a_channel_update_for_an_unknown_channel_starts_the_search() {
    let engine = engine();
    let (device_set, channel, center_hz) = control_channel(&engine);
    let carrier = (device_set, channel);
    let mut follower = searching_follower(
        &engine,
        "tier_three",
        carrier,
        search_over(center_hz),
        Vec::new(),
    );

    follower.observe(&record(
        carrier,
        DvFrame {
            trunk_protocol: Some("tier_three".to_owned()),
            crc_verified: Some(true),
            channel: Some(LOGICAL_CHANNEL),
            slot_activity: vec![sdrmm_wire::DvSlotActivity {
                slot: 1,
                activity: "group voice".to_owned(),
                destination_hash: None,
                destination: Some(9_001),
                logical_channel: Some(LOGICAL_CHANNEL),
            }],
            ..DvFrame::new("DMR", DvFrameKind::Control)
        },
    ));
    follower.reconcile();

    assert_eq!(status(&follower).searching, 1);
    assert_eq!(status(&follower).probes.len(), 4);
}

#[test]
fn a_search_with_no_band_named_reports_the_reach_it_settled_on() {
    let engine = engine();
    let (device_set, channel, _) = control_channel(&engine);
    let carrier = (device_set, channel);
    let mut follower = searching_follower(
        &engine,
        "tier_three",
        carrier,
        TrunkDiscovery {
            enabled: true,
            ranges: Vec::new(),
            max_probes: 4,
        },
        Vec::new(),
    );

    follower.observe(&record(carrier, identified_grant(1)));
    follower.reconcile();

    let status = status(&follower);
    assert!(
        status.candidates > 1,
        "an unnamed search reported {} frequencies",
        status.candidates
    );
}

#[test]
fn a_grant_for_an_unknown_channel_opens_search_receivers() {
    let engine = engine();
    let (device_set, channel, center_hz) = control_channel(&engine);
    let carrier = (device_set, channel);
    let mut follower = searching_follower(
        &engine,
        "tier_three",
        carrier,
        search_over(center_hz),
        Vec::new(),
    );
    assert!(follower.probes.is_empty(), "searched before it was asked");

    follower.observe(&record(carrier, identified_grant(1)));
    follower.reconcile();

    let status = status(&follower);
    assert_eq!(status.searching, 1);
    assert_eq!(
        status.candidates, 4,
        "the status did not say how wide it looks"
    );
    assert_eq!(status.probes.len(), 4, "the raster was not swept");
    assert!(
        status
            .probes
            .iter()
            .all(|probe| probe.freq_hz > center_hz as u64),
        "a search receiver landed on the control channel"
    );
    let set = engine.snapshot().device_sets.remove(0);
    assert_eq!(set.channels.len(), 5);
    assert!(
        set.channels[1..]
            .iter()
            .all(|channel| channel.node.as_deref() == Some("trunk")),
        "a search receiver opened without its system node"
    );
}

#[test]
fn the_control_channel_opens_for_the_system_node() {
    let engine = engine();
    let device_set = engine
        .create_device_set("virtual:band")
        .expect("virtual device");
    let mut follower = follower(&engine, "tier_three", (device_set, u32::MAX));
    follower.controls.clear();
    follower.reconcile();

    let set = engine.snapshot().device_sets.remove(0);
    assert_eq!(set.channels.len(), 1);
    assert_eq!(set.channels[0].node.as_deref(), Some("trunk"));
}

#[test]
fn traffic_that_answers_a_grant_teaches_the_system_where_the_channel_lives() {
    let engine = engine();
    let (device_set, channel, center_hz) = control_channel(&engine);
    let carrier = (device_set, channel);
    let mut follower = searching_follower(
        &engine,
        "tier_three",
        carrier,
        search_over(center_hz),
        Vec::new(),
    );
    follower.observe(&record(carrier, identified_grant(1)));
    follower.reconcile();
    let probe = status(&follower).probes[1];

    for _ in 0..2 {
        follower.observe(&record(
            (probe.device_set, probe.channel),
            traffic(91, 4001, 1),
        ));
    }

    let status = status(&follower);
    assert_eq!(status.channel_map.len(), 1);
    assert_eq!(status.channel_map[0].logical_channel, LOGICAL_CHANNEL);
    assert_eq!(status.channel_map[0].freq_hz, probe.freq_hz);
    assert_eq!(status.channel_map[0].source, TrunkChannelSource::Learned);
    assert_eq!(status.searching, 0);
    assert!(
        !status
            .probes
            .iter()
            .any(|open| open.freq_hz == probe.freq_hz),
        "the search receiver stayed open on a solved channel"
    );
    assert_eq!(status.followers.len(), 1, "the live call was not picked up");
    assert_eq!(status.followers[0].freq_hz, probe.freq_hz);
    assert_eq!(status.followers[0].logical_channel, Some(LOGICAL_CHANNEL));
}

#[test]
fn a_learned_channel_is_followed_without_searching_again() {
    let engine = engine();
    let (device_set, channel, center_hz) = control_channel(&engine);
    let carrier = (device_set, channel);
    let mut follower = searching_follower(
        &engine,
        "tier_three",
        carrier,
        search_over(center_hz),
        Vec::new(),
    );
    follower.observe(&record(carrier, identified_grant(1)));
    follower.reconcile();
    let probe = status(&follower).probes[1];
    for _ in 0..2 {
        follower.observe(&record(
            (probe.device_set, probe.channel),
            traffic(91, 4001, 1),
        ));
    }

    follower.observe(&record(carrier, identified_grant(2)));
    follower.reconcile();

    let status = status(&follower);
    assert_eq!(status.followers.len(), 2);
    assert!(
        status
            .followers
            .iter()
            .all(|open| open.freq_hz == probe.freq_hz)
    );
    assert!(status.probes.is_empty(), "it kept hunting a solved channel");
}

#[test]
fn a_manual_channel_map_is_followed_without_any_search() {
    let engine = engine();
    let (device_set, channel, center_hz) = control_channel(&engine);
    let carrier = (device_set, channel);
    let traffic_hz = center_hz as u64 + 25_000;
    let mut follower = searching_follower(
        &engine,
        "tier_three",
        carrier,
        search_over(center_hz),
        vec![TrunkChannelEntry {
            lcn: LOGICAL_CHANNEL,
            freq_hz: traffic_hz,
        }],
    );

    follower.observe(&record(carrier, identified_grant(1)));
    follower.reconcile();

    let status = status(&follower);
    assert!(status.probes.is_empty(), "it searched for a known channel");
    assert_eq!(status.followers.len(), 1);
    assert_eq!(status.followers[0].freq_hz, traffic_hz);
    assert_eq!(status.channel_map[0].source, TrunkChannelSource::Manual);
}

#[test]
fn an_announced_channel_never_starts_a_search() {
    let engine = engine();
    let (device_set, channel, center_hz) = control_channel(&engine);
    let carrier = (device_set, channel);
    let mut follower = searching_follower(
        &engine,
        "tier_three",
        carrier,
        search_over(center_hz),
        Vec::new(),
    );

    follower.observe(&record(carrier, definition(center_hz as u64 + 12_500)));
    follower.observe(&record(carrier, identified_grant(1)));
    follower.reconcile();

    let status = status(&follower);
    assert!(status.probes.is_empty());
    assert_eq!(status.searching, 0);
    assert_eq!(status.channel_map.len(), 1);
    assert_eq!(status.channel_map[0].source, TrunkChannelSource::Announced);
}

#[test]
fn dropping_the_system_closes_the_search_receivers_it_opened() {
    let engine = engine();
    let (device_set, channel, center_hz) = control_channel(&engine);
    let carrier = (device_set, channel);
    let mut follower = searching_follower(
        &engine,
        "tier_three",
        carrier,
        search_over(center_hz),
        Vec::new(),
    );
    follower.observe(&record(carrier, identified_grant(1)));
    follower.reconcile();
    assert_eq!(engine.snapshot().device_sets[0].channels.len(), 5);

    follower.systems.clear();
    follower.reconcile();

    assert_eq!(
        engine.snapshot().device_sets[0].channels.len(),
        0,
        "orphaned search receivers were left running"
    );
    assert!(follower.probes.is_empty());
}

#[test]
fn a_search_receiver_that_finds_a_control_channel_is_not_mistaken_for_traffic() {
    let engine = engine();
    let (device_set, channel, center_hz) = control_channel(&engine);
    let carrier = (device_set, channel);
    let mut follower = searching_follower(
        &engine,
        "tier_three",
        carrier,
        search_over(center_hz),
        Vec::new(),
    );
    follower.observe(&record(carrier, identified_grant(1)));
    follower.reconcile();
    let probe = status(&follower).probes[0];

    for _ in 0..2 {
        let mut announced = identified_grant(1);
        announced.control_channel = Some(true);
        follower.observe(&record((probe.device_set, probe.channel), announced));
    }
    follower.reconcile();

    let status = status(&follower);
    assert!(status.channel_map.is_empty(), "a control channel was bound");
    assert!(
        !status
            .probes
            .iter()
            .any(|open| open.freq_hz == probe.freq_hz),
        "a known control channel stayed in the search"
    );
}

fn ruled_follower(
    engine: &Arc<Engine>,
    carrier: (u32, u32),
    talkgroups: TrunkTalkgroups,
) -> Follower {
    let mut follower = follower(engine, "tier_three", carrier);
    follower.systems[0].talkgroups = talkgroups;
    follower
}

fn grant_to(logical_channel: u16, slot: u8, destination: u32) -> DvFrame {
    DvFrame {
        channel: Some(logical_channel),
        destination: Some(destination),
        group_call: Some(true),
        ..grant(slot)
    }
}

fn define(follower: &mut Follower, carrier: (u32, u32), logical_channel: u16, rx_hz: u64) {
    let mut frame = definition(rx_hz);
    if let Some(definition) = &mut frame.channel_definition {
        definition.channel = logical_channel;
    }
    follower.observe(&record(carrier, frame));
}

/// Fills every receiver the system may hold with a call on its own channel and slot.
fn fill(follower: &mut Follower, carrier: (u32, u32), center_hz: f64) {
    for logical_channel in 1..=(MAX_FOLLOWERS_PER_SYSTEM / 2) as u16 {
        let rx_hz = center_hz as u64 + u64::from(logical_channel) * 12_500;
        define(follower, carrier, logical_channel, rx_hz);
        for slot in [1, 2] {
            let destination = 100 + u32::from(logical_channel) * 2 + u32::from(slot);
            follower.observe(&record(
                carrier,
                grant_to(logical_channel, slot, destination),
            ));
        }
    }
    assert_eq!(follower.followers.len(), MAX_FOLLOWERS_PER_SYSTEM);
}

fn age_every_grant(follower: &mut Follower) {
    let past = Instant::now()
        .checked_sub(GRANT_HOLD)
        .expect("a clock older than the grant hold");
    for open in follower.followers.values_mut() {
        open.granted = past;
    }
}

#[test]
fn a_locked_out_talkgroup_is_never_followed() {
    let engine = engine();
    let (device_set, channel, center_hz) = control_channel(&engine);
    let carrier = (device_set, channel);
    let mut follower = ruled_follower(
        &engine,
        carrier,
        TrunkTalkgroups {
            lockout: vec![91],
            ..TrunkTalkgroups::default()
        },
    );

    follower.observe(&record(carrier, definition(center_hz as u64 + 125_000)));
    follower.observe(&record(carrier, grant(1)));

    assert!(follower.followers.is_empty());
}

#[test]
fn listed_only_follows_just_the_priority_talkgroups() {
    let engine = engine();
    let (device_set, channel, center_hz) = control_channel(&engine);
    let carrier = (device_set, channel);
    let mut follower = ruled_follower(
        &engine,
        carrier,
        TrunkTalkgroups {
            priority: vec![92],
            listed_only: true,
            ..TrunkTalkgroups::default()
        },
    );

    follower.observe(&record(carrier, definition(center_hz as u64 + 125_000)));
    follower.observe(&record(carrier, grant_to(LOGICAL_CHANNEL, 1, 91)));
    follower.observe(&record(carrier, grant_to(LOGICAL_CHANNEL, 2, 92)));

    let status = status(&follower);
    assert_eq!(status.followers.len(), 1);
    assert_eq!(status.followers[0].destination, Some(92));
}

#[test]
fn a_follower_names_the_call_its_channel_was_granted_to() {
    let engine = engine();
    let (device_set, channel, center_hz) = control_channel(&engine);
    let carrier = (device_set, channel);
    let mut follower = follower(&engine, "tier_three", carrier);

    follower.observe(&record(carrier, definition(center_hz as u64 + 125_000)));
    follower.observe(&record(carrier, identified_grant(1)));
    follower.observe(&record(carrier, grant_to(LOGICAL_CHANNEL, 1, 93)));

    let status = status(&follower);
    assert_eq!(status.followers.len(), 1);
    assert_eq!(status.followers[0].destination, Some(93));
    assert_eq!(status.followers[0].group_call, Some(true));
}

#[test]
fn a_full_system_hands_an_idle_receiver_to_a_new_call() {
    let engine = engine();
    let (device_set, channel, center_hz) = control_channel(&engine);
    let carrier = (device_set, channel);
    let mut follower = follower(&engine, "tier_three", carrier);
    fill(&mut follower, carrier, center_hz);
    age_every_grant(&mut follower);

    define(&mut follower, carrier, 40, center_hz as u64 + 125_000);
    follower.observe(&record(carrier, grant_to(40, 1, 7)));

    let status = status(&follower);
    assert_eq!(status.followers.len(), MAX_FOLLOWERS_PER_SYSTEM);
    assert!(
        status
            .followers
            .iter()
            .any(|open| open.destination == Some(7))
    );
    assert!(status.problems.is_empty());
}

#[test]
fn a_priority_call_takes_a_busy_receiver_from_an_ordinary_one() {
    let engine = engine();
    let (device_set, channel, center_hz) = control_channel(&engine);
    let carrier = (device_set, channel);
    let mut follower = ruled_follower(
        &engine,
        carrier,
        TrunkTalkgroups {
            priority: vec![7],
            ..TrunkTalkgroups::default()
        },
    );
    fill(&mut follower, carrier, center_hz);
    define(&mut follower, carrier, 40, center_hz as u64 + 125_000);
    define(&mut follower, carrier, 41, center_hz as u64 + 137_500);

    follower.observe(&record(carrier, grant_to(41, 1, 8)));
    follower.observe(&record(carrier, grant_to(40, 1, 7)));

    let status = status(&follower);
    assert_eq!(status.followers.len(), MAX_FOLLOWERS_PER_SYSTEM);
    assert!(
        status
            .followers
            .iter()
            .any(|open| open.destination == Some(7))
    );
    assert!(
        !status
            .followers
            .iter()
            .any(|open| open.destination == Some(8))
    );
    assert_eq!(
        status.problems.len(),
        1,
        "the ordinary call was not reported"
    );
}

#[test]
fn a_control_burst_names_the_site() {
    let engine = engine();
    let (device_set, channel, _) = control_channel(&engine);
    let carrier = (device_set, channel);
    let mut follower = follower(&engine, "tier_three", carrier);

    follower.observe(&record(
        carrier,
        DvFrame {
            system_id: Some(0x1234),
            network_id: Some(12),
            site_id: Some(3),
            ..grant(1)
        },
    ));

    let site = status(&follower).site;
    assert_eq!(site.system_id, Some(0x1234));
    assert_eq!(site.network_id, Some(12));
    assert_eq!(site.site_id, Some(3));
}

#[test]
fn a_protocol_the_decoder_never_declared_is_not_taken_for_the_system() {
    let engine = engine();
    let (device_set, channel, _) = control_channel(&engine);
    let carrier = (device_set, channel);
    let mut follower = follower(&engine, AUTO_TRUNK_PROTOCOL, carrier);

    follower.observe(&record(
        carrier,
        DvFrame {
            trunk_protocol: Some("phase_two".to_owned()),
            ..DvFrame::new("DMR", DvFrameKind::Control)
        },
    ));

    assert_eq!(status(&follower).detected, None);
}

#[test]
fn a_quiet_control_channel_with_nowhere_else_to_go_stays_put() {
    let engine = engine();
    let (device_set, channel, _) = control_channel(&engine);
    let carrier = (device_set, channel);
    let mut follower = follower(&engine, "tier_three", carrier);
    let configured = follower.systems[0].radio.map(|radio| radio.control_hz);
    let silent = Instant::now()
        .checked_sub(CONTROL_SILENCE)
        .expect("a clock older than the silence");
    if let Some(control) = follower.controls.get_mut("trunk") {
        control.heard = silent;
    }

    follower.reconcile();

    assert_eq!(
        status(&follower).control.map(|control| control.freq_hz),
        configured
    );
}

#[test]
fn a_silent_control_channel_steps_round_the_sites_others() {
    assert_eq!(next_control(10, 10, &[20, 30]), Some(20));
    assert_eq!(next_control(10, 20, &[20, 30]), Some(30));
    assert_eq!(next_control(10, 30, &[20, 30]), Some(10));
    assert_eq!(next_control(10, 10, &[10]), None);
    assert_eq!(next_control(10, 10, &[]), None);
}

#[test]
fn a_protocol_with_one_call_per_carrier_follows_its_grant_without_a_timeslot() {
    let engine = engine();
    let p25 = ChannelParams::new("p25", &P25Params::default()).expect("p25 settings");
    let (device_set, channel, center_hz) = control_channel_of(&engine, p25.clone());
    let carrier = (device_set, channel);
    let mut follower = follower(&engine, "p25", carrier);
    follower.systems[0].decoder = "p25".to_owned();
    follower.reconcile();
    let traffic = center_hz as u64 + 125_000;
    let logical_channel = 1 << 12 | 10;

    follower.observe(&record(
        carrier,
        DvFrame {
            trunk_protocol: Some("p25".to_owned()),
            channel: Some(logical_channel),
            destination: Some(0x1201),
            source: Some(0xABCDEF),
            group_call: Some(true),
            channel_definition: Some(DvChannelDefinition {
                channel: logical_channel,
                tx_hz: traffic - 45_000_000,
                rx_hz: traffic,
                color_code: None,
            }),
            ..DvFrame::new("P25", DvFrameKind::Control)
        },
    ));

    let set = engine.snapshot().device_sets.remove(0);
    assert_eq!(set.channels.len(), 2, "the follower was not created");
    assert_eq!(set.channels[1].settings.frequency_hz, traffic as f64);
    assert_eq!(set.channels[1].settings.params, p25);
    let status = status(&follower);
    assert_eq!(status.followers.len(), 1);
    assert_eq!(status.followers[0].slot, None);
    assert_eq!(status.followers[0].logical_channel, Some(logical_channel));
    assert_eq!(status.followers[0].destination, Some(0x1201));
    assert!(status.problems.is_empty(), "{:?}", status.problems);
}

#[test]
fn a_site_code_wider_than_a_dmr_colour_code_keys_the_site() {
    let engine = engine();
    let p25 = ChannelParams::new("p25", &P25Params::default()).expect("p25 settings");
    let (device_set, channel, _) = control_channel_of(&engine, p25);
    let carrier = (device_set, channel);
    let mut follower = follower(&engine, "p25", carrier);
    follower.systems[0].decoder = "p25".to_owned();
    follower.reconcile();

    follower.observe(&record(
        carrier,
        DvFrame {
            trunk_protocol: Some("p25".to_owned()),
            color_code: Some(0x4D2),
            ..DvFrame::new("P25", DvFrameKind::Control)
        },
    ));

    follower.publish();
    assert_eq!(status(&follower).color_code, Some(0x4D2));
}
