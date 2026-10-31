use std::sync::Mutex;

use sdrmm_wire::{
    ChannelNode, NodeBody, PatchEdge, PatchGraph, PatchNode, PortRef, Position, RotatorNode, Runs,
    SatelliteLook, SatelliteNode, ScannerNode, SkyTrack, Tap,
    patch::resolve::{ResolveCtx, compile},
};

use super::*;

#[derive(Default)]
struct FakeRig {
    calls: Mutex<Vec<String>>,
    lanes: Mutex<Vec<WatchedLane>>,
}

impl FakeRig {
    fn calls(&self) -> Vec<String> {
        locked(&self.calls).clone()
    }
}

impl Rig for FakeRig {
    fn tune(&self, at: ChannelAddr, freq_hz: f64) -> Result<(), String> {
        locked(&self.calls).push(format!("tune {} {freq_hz}", at.channel));
        Ok(())
    }

    fn steer(&self, at: ChannelAddr, doppler: DopplerSteer) -> Result<(), String> {
        locked(&self.calls).push(format!(
            "steer {} {} {}",
            at.channel, doppler.shift_hz, doppler.rate_hz_s
        ));
        Ok(())
    }

    fn halt(&self, at: ChannelAddr, from: &str) -> Result<(), String> {
        locked(&self.calls).push(format!("halt {} {from}", at.channel));
        Ok(())
    }

    fn sweep(&self, at: ChannelAddr, sweep: &Sweep<'_>) -> Result<Option<SweepStatus>, String> {
        locked(&self.lanes).push(sweep.lane);
        if sweep.lane.stream == at.channel {
            Ok(None)
        } else {
            Err("Watches another lane".to_owned())
        }
    }
}

fn node(id: &str, body: NodeBody) -> PatchNode {
    PatchNode {
        id: id.to_owned(),
        body,
        position: Position { x: 0.0, y: 0.0 },
        size: None,
        label: None,
    }
}

fn wire(from: (&str, &str), to: (&str, &str)) -> PatchEdge {
    PatchEdge {
        from: PortRef {
            node: from.0.to_owned(),
            port: from.1.to_owned(),
        },
        to: PortRef {
            node: to.0.to_owned(),
            port: to.1.to_owned(),
        },
    }
}

fn channel(id: &str) -> PatchNode {
    node(
        id,
        NodeBody::Channel(ChannelNode {
            channel_type: "nfm".to_owned(),
            tuning_locked: false,
        }),
    )
}

fn resolve(nodes: Vec<PatchNode>, edges: Vec<PatchEdge>) -> ResolvedPatch {
    let mut patch =
        compile(&PatchGraph { nodes, edges }, &ResolveCtx::default()).expect("compiles");
    for (index, node) in patch.nodes.iter_mut().enumerate() {
        if node.kind == sdrmm_wire::NodeKind::Channel {
            node.runs = Runs::Channel {
                device_set: 1,
                channel: u32::try_from(index).unwrap_or_default(),
                stream: 0,
            };
        }
    }
    patch
}

fn sky() -> ControlCommand {
    ControlCommand::Point(SkyTrack {
        look: SatelliteLook {
            azimuth_deg: 90.0,
            elevation_deg: 20.0,
            range_km: 1_000.0,
            range_rate_km_s: 1.0,
        },
        visible: true,
        next_pass: None,
        pass_track: Vec::new(),
    })
}

#[test]
fn a_rotator_follows_only_its_wired_satellite() {
    let nodes = || {
        vec![
            node("wired", NodeBody::Satellite(SatelliteNode::default())),
            node("loose", NodeBody::Satellite(SatelliteNode::default())),
            node("dish", NodeBody::Rotator(RotatorNode::default())),
        ]
    };
    let patch = resolve(
        nodes(),
        vec![wire(("wired", "control"), ("dish", "control"))],
    );
    let bus = ControlBus::default();
    let rig = FakeRig::default();
    let dish = bus.follow("dish");
    assert!(bus.deliver(&rig, &patch, "loose", &sky()).is_empty());
    assert_eq!(*dish.borrow(), None);
    let acks = bus.deliver(&rig, &patch, "wired", &sky());
    assert_eq!(acks.len(), 1);
    assert_eq!(acks[0].target, "dish");
    assert_eq!(acks[0].verb, Some(ControlVerb::Point));
    assert_eq!(*dish.borrow(), Some(sky()));
    let unwired = resolve(nodes(), Vec::new());
    assert!(bus.deliver(&rig, &unwired, "wired", &sky()).is_empty());
    assert_eq!(*dish.borrow(), None, "unwiring releases the rotator");
}

#[test]
fn an_unwired_satellite_steers_nothing_and_the_doppler_rate_passes() {
    let nodes = vec![
        node("sat", NodeBody::Satellite(SatelliteNode::default())),
        node("loose", NodeBody::Satellite(SatelliteNode::default())),
        channel("voice"),
    ];
    let patch = resolve(nodes, vec![wire(("sat", "control"), ("voice", "control"))]);
    let bus = ControlBus::default();
    let rig = FakeRig::default();
    let doppler = ControlCommand::Doppler(DopplerSteer {
        shift_hz: -2_000.0,
        rate_hz_s: 12.5,
    });
    assert!(bus.deliver(&rig, &patch, "loose", &doppler).is_empty());
    assert!(rig.calls().is_empty());
    assert!(
        bus.deliver(&rig, &patch, "sat", &sky()).is_empty(),
        "a channel takes no Point"
    );
    let acks = bus.deliver(&rig, &patch, "sat", &doppler);
    assert_eq!(acks.len(), 1);
    assert_eq!(acks[0].error, None);
    assert_eq!(rig.calls(), vec!["steer 2 -2000 12.5".to_owned()]);
    bus.release_all_but(&rig, &patch, &|_: &str| false);
    assert_eq!(rig.calls().last().map(String::as_str), Some("steer 2 0 0"));
}

#[test]
fn a_target_error_returns_in_the_acks() {
    let nodes = vec![
        node("sat", NodeBody::Satellite(SatelliteNode::default())),
        channel("voice"),
    ];
    let mut patch = resolve(nodes, vec![wire(("sat", "control"), ("voice", "control"))]);
    for node in &mut patch.nodes {
        if node.id == "voice" {
            node.runs = Runs::Unbound {
                reason: sdrmm_wire::UnboundReason::RadioClosed,
            };
        }
    }
    let acks = ControlBus::default().deliver(
        &FakeRig::default(),
        &patch,
        "sat",
        &ControlCommand::Tune { freq_hz: 145.8e6 },
    );
    assert_eq!(acks.len(), 1);
    assert_eq!(acks[0].target, "voice");
    assert_eq!(acks[0].error.as_deref(), Some(NOT_RUNNING));
}

#[test]
fn a_hand_lock_beats_the_control_wire() {
    let mut voice = channel("voice");
    if let NodeBody::Channel(locked) = &mut voice.body {
        locked.tuning_locked = true;
    }
    let nodes = vec![
        node("sat", NodeBody::Satellite(SatelliteNode::default())),
        voice,
    ];
    let patch = resolve(nodes, vec![wire(("sat", "control"), ("voice", "control"))]);
    let bus = ControlBus::default();
    let rig = FakeRig::default();
    let acks = bus.deliver(
        &rig,
        &patch,
        "sat",
        &ControlCommand::Tune { freq_hz: 145.8e6 },
    );
    assert_eq!(acks[0].error.as_deref(), Some("Tuning locked"));
    assert!(rig.calls().is_empty());
    let acks = bus.deliver(&rig, &patch, "sat", &ControlCommand::Release);
    assert_eq!(acks[0].error, None, "a lock never blocks a release");
}

#[test]
fn a_sweep_carries_the_lane_the_scanner_watches() {
    let nodes = vec![
        node("radio", NodeBody::Device(sdrmm_wire::DeviceNode::default())),
        node("scan", NodeBody::Scanner(ScannerNode::default())),
        channel("voice"),
    ];
    let edges = vec![
        wire(("radio", "iq"), ("voice", "iq")),
        wire(("scan", "control"), ("voice", "control")),
    ];
    let mut patch = resolve(nodes, edges.clone());
    let job = ControlCommand::Sweep(SweepJob::Scan {
        settings: ScanSettings::default(),
        action: ScanAction::Start,
    });
    let bus = ControlBus::default();
    let rig = FakeRig::default();
    let acks = bus.deliver(&rig, &patch, "scan", &job);
    assert_eq!(acks[0].error.as_deref(), Some(NO_LANE));

    let mut watched = edges;
    watched.push(wire(("radio", "iq"), ("scan", "iq")));
    patch = resolve(
        vec![
            node("radio", NodeBody::Device(sdrmm_wire::DeviceNode::default())),
            node("scan", NodeBody::Scanner(ScannerNode::default())),
            channel("voice"),
        ],
        watched,
    );
    let lane = |stream| {
        let mut patch = patch.clone();
        for node in &mut patch.nodes {
            for port in &mut node.ports {
                for feed in &mut port.feeds {
                    feed.tap = Some(Tap::Lane {
                        device_set: 1,
                        stream,
                    });
                }
            }
        }
        patch
    };
    let refused = bus.deliver(&rig, &lane(0), "scan", &job);
    assert_eq!(refused[0].error.as_deref(), Some("Watches another lane"));
    let taken = bus.deliver(&rig, &lane(2), "scan", &job);
    assert_eq!(taken[0].error, None);
    assert_eq!(
        locked(&rig.lanes).last(),
        Some(&WatchedLane {
            device_set: 1,
            stream: 2,
        })
    );
}

#[test]
fn unwiring_a_scanner_stops_the_scan_it_started() {
    let nodes = || {
        vec![
            node("radio", NodeBody::Device(sdrmm_wire::DeviceNode::default())),
            node("scan", NodeBody::Scanner(ScannerNode::default())),
            channel("voice"),
        ]
    };
    let watched = vec![
        wire(("radio", "iq"), ("voice", "iq")),
        wire(("radio", "iq"), ("scan", "iq")),
    ];
    let mut wired = watched.clone();
    wired.push(wire(("scan", "control"), ("voice", "control")));
    let mut patch = resolve(nodes(), wired);
    for node in &mut patch.nodes {
        for port in &mut node.ports {
            for feed in &mut port.feeds {
                feed.tap = Some(Tap::Lane {
                    device_set: 1,
                    stream: 2,
                });
            }
        }
    }
    let bus = ControlBus::default();
    let rig = FakeRig::default();
    let start = ControlCommand::Sweep(SweepJob::Scan {
        settings: ScanSettings::default(),
        action: ScanAction::Start,
    });
    assert_eq!(bus.deliver(&rig, &patch, "scan", &start)[0].error, None);
    bus.settle(&rig, &patch);
    assert!(rig.calls().is_empty(), "still wired, still scanning");

    bus.settle(&rig, &resolve(nodes(), watched));
    assert_eq!(rig.calls(), vec!["halt 2 scan".to_owned()]);
    bus.settle(&rig, &resolve(nodes(), Vec::new()));
    assert_eq!(rig.calls().len(), 1, "stopped once");
}

#[test]
fn removing_a_scanned_channel_still_stops_its_scan() {
    let nodes = vec![
        node("radio", NodeBody::Device(sdrmm_wire::DeviceNode::default())),
        node("scan", NodeBody::Scanner(ScannerNode::default())),
        channel("voice"),
    ];
    let watched = vec![
        wire(("radio", "iq"), ("voice", "iq")),
        wire(("radio", "iq"), ("scan", "iq")),
    ];
    let mut wired = watched.clone();
    wired.push(wire(("scan", "control"), ("voice", "control")));
    let mut patch = resolve(nodes.clone(), wired);
    for feed in patch
        .nodes
        .iter_mut()
        .flat_map(|node| &mut node.ports)
        .flat_map(|port| &mut port.feeds)
    {
        feed.tap = Some(Tap::Lane {
            device_set: 1,
            stream: 2,
        });
    }
    let bus = ControlBus::default();
    let rig = FakeRig::default();
    let start = ControlCommand::Sweep(SweepJob::Scan {
        settings: ScanSettings::default(),
        action: ScanAction::Start,
    });
    assert_eq!(bus.deliver(&rig, &patch, "scan", &start)[0].error, None);

    let without_voice = nodes[..2].to_vec();
    bus.settle(&rig, &resolve(without_voice, vec![watched[1].clone()]));
    assert_eq!(rig.calls(), vec!["halt 2 scan".to_owned()]);
}

#[test]
fn a_satellite_steering_leaves_a_running_scan_alone() {
    let nodes = vec![
        node("radio", NodeBody::Device(sdrmm_wire::DeviceNode::default())),
        node("scan", NodeBody::Scanner(ScannerNode::default())),
        node("sat", NodeBody::Satellite(SatelliteNode::default())),
        channel("voice"),
        channel("downlink"),
    ];
    let edges = vec![
        wire(("radio", "iq"), ("voice", "iq")),
        wire(("radio", "iq"), ("scan", "iq")),
        wire(("scan", "control"), ("voice", "control")),
        wire(("sat", "control"), ("downlink", "control")),
    ];
    let mut patch = resolve(nodes, edges);
    for node in &mut patch.nodes {
        for port in &mut node.ports {
            for feed in &mut port.feeds {
                feed.tap = Some(Tap::Lane {
                    device_set: 1,
                    stream: 3,
                });
            }
        }
    }
    let bus = ControlBus::default();
    let rig = FakeRig::default();
    let start = ControlCommand::Sweep(SweepJob::Scan {
        settings: ScanSettings::default(),
        action: ScanAction::Start,
    });
    assert_eq!(bus.deliver(&rig, &patch, "scan", &start)[0].error, None);
    for _ in 0..3 {
        bus.deliver(
            &rig,
            &patch,
            "sat",
            &ControlCommand::Doppler(DopplerSteer::default()),
        );
        bus.settle(&rig, &patch);
    }
    assert!(
        rig.calls().iter().all(|call| !call.starts_with("halt")),
        "{:?}",
        rig.calls()
    );
}

#[test]
fn a_radio_runs_while_its_satellite_says_on_and_rests_on_release() {
    let nodes = vec![
        node("sat", NodeBody::Satellite(SatelliteNode::default())),
        node("dev", NodeBody::Device(sdrmm_wire::DeviceNode::default())),
        channel("voice"),
    ];
    let edges = vec![
        wire(("sat", "control"), ("dev", "control")),
        wire(("sat", "control"), ("voice", "control")),
    ];
    let graph = PatchGraph {
        nodes: nodes.clone(),
        edges: edges.clone(),
    };
    let patch = resolve(nodes, edges);
    let runs = Arc::new(RunGate::default());
    let bus = ControlBus::gated_by(runs.clone());
    let rig = FakeRig::default();
    let resting = |graph: &PatchGraph| {
        runs.gated(graph)
            .node("dev")
            .is_some_and(|node| matches!(&node.body, NodeBody::Device(device) if device.waiting))
    };
    let acks = bus.deliver(&rig, &patch, "sat", &ControlCommand::Run { on: true });
    assert_eq!(acks.len(), 1, "a decoder takes no run command");
    assert_eq!(acks[0].target, "dev");
    assert!(!resting(&graph));
    bus.deliver(&rig, &patch, "sat", &ControlCommand::Release);
    assert!(resting(&graph));
}
