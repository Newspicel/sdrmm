use sdrmm_wire::{HuntNode, ScanSettings, ScannerNode};

use super::*;

fn ack(error: Option<&str>) -> ControlAck {
    ControlAck {
        target: "ch".to_owned(),
        verb: Some(ControlVerb::Sweep),
        error: error.map(str::to_owned),
        sweep: None,
        outranked: false,
    }
}

#[test]
fn a_decoder_on_its_way_is_no_fault() {
    assert_eq!(fault(&[ack(Some(control::NOT_RUNNING))]), None);
    assert_eq!(fault(&[ack(None)]), None);
    assert_eq!(
        fault(&[ack(Some("Add a range"))]).as_deref(),
        Some("Add a range")
    );
}

#[test]
fn a_scanner_keeps_its_run_and_lockouts() {
    let mut body = NodeBody::Scanner(ScannerNode::default());
    assert!(kept(&mut body, Some(true), Some(&[1.0e8])));
    let NodeBody::Scanner(scanner) = &body else {
        panic!("still a scanner");
    };
    assert!(scanner.running);
    assert_eq!(scanner.settings.lockouts, [1.0e8]);
    assert!(!kept(&mut body, Some(true), Some(&[1.0e8])), "nothing new");
}

#[test]
fn a_hunt_keeps_its_run() {
    let mut body = NodeBody::Hunt(HuntNode::default());
    assert!(kept(&mut body, Some(true), None));
    assert_eq!(running(&body), Some(true));
    assert!(kept(&mut body, Some(false), None));
    assert_eq!(running(&body), Some(false));
}

#[test]
fn only_sweeping_nodes_take_a_sweep() {
    let scanner = NodeBody::Scanner(ScannerNode {
        settings: ScanSettings::default(),
        running: true,
    });
    let start = || action(&scanner, true).expect("a scanner starts");
    assert!(command(start(), &scanner).is_some());
    assert!(command(start(), &NodeBody::Map).is_none());
    assert_eq!(running(&NodeBody::Map), None);
}
