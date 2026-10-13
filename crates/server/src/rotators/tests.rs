use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
};

use sdrmm_wire::{
    GpsNode, PassPoint, PatchEdge, PatchGraph, PatchNode, PortRef, Position, SatelliteLook,
    SatelliteNode, SatellitePass, SkyPoint, SkyTrack,
};

use super::{
    driver::{Aim, aim, needs_move},
    plan::Mount,
    *,
};

fn node(id: &str, body: NodeBody) -> PatchNode {
    PatchNode {
        id: id.to_owned(),
        body,
        position: Position { x: 0.0, y: 0.0 },
        size: None,
        label: None,
    }
}

fn control(from: &str, to: &str) -> PatchEdge {
    PatchEdge {
        from: PortRef {
            node: from.to_owned(),
            port: "control".to_owned(),
        },
        to: PortRef {
            node: to.to_owned(),
            port: "control".to_owned(),
        },
    }
}

fn resolved(graph: PatchGraph) -> Resolved {
    Resolved {
        patch: sdrmm_wire::patch::resolve::compile(&graph, &Default::default()).expect("compiles"),
        graph: std::sync::Arc::new(graph),
        ..Resolved::default()
    }
}

fn rotctld_link(address: &str) -> RotatorLink {
    RotatorLink::Rotctld {
        address: address.to_owned(),
    }
}

fn sky(azimuth_deg: f64, elevation_deg: f64) -> SkyPoint {
    SkyPoint {
        azimuth_deg,
        elevation_deg,
    }
}

const PLAIN: Mount = Mount {
    azimuth_max_deg: 360.0,
    elevation_max_deg: 90.0,
    flips: false,
};

#[test]
fn only_linked_rotators_run_and_each_follows_its_wired_satellite() {
    let linked = RotatorNode {
        link: Some(rotctld_link("127.0.0.1:4533")),
        ..RotatorNode::default()
    };
    let graph = PatchGraph {
        nodes: vec![
            node("sat", NodeBody::Satellite(SatelliteNode::default())),
            node("gps", NodeBody::Gps(GpsNode::default())),
            node("dish", NodeBody::Rotator(linked.clone())),
            node("spare", NodeBody::Rotator(linked)),
            node("blank", NodeBody::Rotator(RotatorNode::default())),
        ],
        edges: vec![control("sat", "dish")],
    };
    let found = wanted(&resolved(graph));
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].node, "dish");
    assert!(found[0].follow.wired);
    assert!(!found[1].follow.wired);
}

#[test]
fn a_visible_satellite_is_tracked_and_a_set_one_waits_at_its_rise() {
    let mut status = SkyTrack {
        look: SatelliteLook {
            azimuth_deg: 120.0,
            elevation_deg: 35.0,
            range_km: 900.0,
            range_rate_km_s: -3.0,
        },
        visible: true,
        next_pass: Some(SatellitePass {
            aos: Some(100),
            los: Some(700),
            max_elevation_deg: 60.0,
            max_at: 400,
        }),
        pass_track: vec![PassPoint {
            at: 100,
            azimuth_deg: 310.0,
            elevation_deg: 0.0,
        }],
    };
    let here = sky(0.0, 0.0);
    assert_eq!(
        aim(&status, None, PLAIN, here, 50),
        Some(Aim {
            target: sky(120.0, 35.0),
            awaiting_pass: false,
            flipped: false,
        })
    );
    status.visible = false;
    assert_eq!(
        aim(&status, None, PLAIN, here, 50),
        Some(Aim {
            target: sky(310.0, 0.0),
            awaiting_pass: true,
            flipped: false,
        })
    );
    status.pass_track.clear();
    assert_eq!(aim(&status, None, PLAIN, here, 50), None);
}

#[test]
fn small_moves_inside_the_tolerance_are_held_back() {
    let sent = sky(359.5, 10.0);
    assert!(needs_move(None, sent, 2.0));
    assert!(!needs_move(Some(sent), sky(360.5, 11.0), 2.0));
    assert!(needs_move(Some(sent), sky(0.5, 10.0), 2.0));
    assert!(needs_move(Some(sent), sky(359.5, 12.5), 2.0));
}

#[test]
fn a_new_axis_setup_reconnects_but_a_new_mode_does_not() {
    let settings = RotatorNode {
        link: Some(rotctld_link("127.0.0.1:4533")),
        ..RotatorNode::default()
    };
    let graph = |settings: RotatorNode| PatchGraph {
        nodes: vec![node("dish", NodeBody::Rotator(settings))],
        edges: Vec::new(),
    };
    let before = wanted(&resolved(graph(settings.clone())))[0]
        .connection
        .clone();
    let parked = wanted(&resolved(graph(RotatorNode {
        mode: sdrmm_wire::RotatorMode::Park,
        ..settings.clone()
    })))[0]
        .connection
        .clone();
    let turntable = wanted(&resolved(graph(RotatorNode {
        axes: RotatorAxes::Azimuth,
        ..settings
    })))[0]
        .connection
        .clone();
    assert_eq!(before, parked);
    assert_ne!(before, turntable);
}

#[test]
fn a_rotctld_link_reads_points_and_stops_over_tcp() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("binds");
    let address = listener.local_addr().expect("bound").to_string();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accepts");
        let mut reader = BufReader::new(stream.try_clone().expect("clones"));
        let mut writer = stream;
        let mut heard = Vec::new();
        for _ in 0..3 {
            let mut line = String::new();
            reader.read_line(&mut line).expect("reads");
            let reply = if line.starts_with('p') {
                "10.000000\n5.000000\n"
            } else {
                "RPRT 0\n"
            };
            writer.write_all(reply.as_bytes()).expect("writes");
            heard.push(line);
        }
        heard
    });
    let mut link =
        link::open(&rotctld_link(&address), RotatorAxes::AzimuthElevation).expect("connects");
    assert_eq!(link.position().expect("reads"), sky(10.0, 5.0));
    link.point(sky(200.0, 30.0)).expect("points");
    link.stop().expect("stops");
    assert_eq!(
        server.join().expect("serves"),
        vec![
            "p\n".to_owned(),
            "P 200.0 30.0\n".to_owned(),
            "S\n".to_owned()
        ]
    );
}
