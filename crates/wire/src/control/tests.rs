use super::*;

#[test]
fn commands_name_their_verb_and_keep_the_doppler_rate() {
    let doppler = ControlCommand::Doppler(DopplerSteer {
        shift_hz: -1_200.0,
        rate_hz_s: 8.5,
    });
    assert_eq!(doppler.verb(), Some(ControlVerb::Doppler));
    assert_eq!(ControlCommand::Release.verb(), None);
    let json = serde_json::to_value(&doppler).expect("json");
    assert_eq!(json["command"], "doppler");
    assert_eq!(json["with"]["rate_hz_s"], 8.5);
    assert_eq!(
        serde_json::from_value::<ControlCommand>(json).expect("back"),
        doppler
    );
}

#[test]
fn a_sweep_job_is_tagged_by_job() {
    let sweep = ControlCommand::Sweep(SweepJob::Hunt {
        params: HuntSweepParams::default(),
        action: HuntAction::Start,
    });
    let json = serde_json::to_value(&sweep).expect("json");
    assert_eq!(json["with"]["job"], "hunt");
    assert_eq!(json["with"]["action"], "start");
}
