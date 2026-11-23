use sdrmm_wire::{SatelliteLook, SatellitePass};

use super::*;

fn pass(visible: bool, aos: i64, peak_deg: f64) -> SkyTrack {
    SkyTrack {
        next_pass: Some(SatellitePass {
            aos: Some(aos),
            los: Some(aos + 600),
            max_elevation_deg: peak_deg,
            max_at: aos + 300,
        }),
        ..passless(visible, if visible { 10.0 } else { -10.0 })
    }
}

fn passless(visible: bool, elevation_deg: f64) -> SkyTrack {
    SkyTrack {
        name: None,
        look: SatelliteLook {
            azimuth_deg: 90.0,
            elevation_deg,
            range_km: 1_000.0,
            range_rate_km_s: 1.0,
        },
        visible,
        next_pass: None,
        pass_track: Vec::new(),
    }
}

fn from(lead: Option<Lead>) -> Option<String> {
    lead.map(|lead| lead.from)
}

#[test]
fn the_pass_that_climbs_highest_leads() {
    let mut pointing = Pointing::default();
    pointing.bid("dish", "first", &pass(true, 0, 10.0));
    pointing.bid("dish", "mid", &pass(true, 0, 30.0));
    pointing.bid("dish", "high", &pass(true, 0, 70.0));
    assert_eq!(
        from(pointing.bid("dish", "first", &pass(false, 5_000, 10.0))),
        Some("high".to_owned())
    );
}

#[test]
fn a_pass_in_view_keeps_the_dish_until_it_sets() {
    let mut pointing = Pointing::default();
    pointing.bid("dish", "first", &pass(true, 0, 20.0));
    assert_eq!(
        from(pointing.bid("dish", "better", &pass(true, 60, 80.0))),
        Some("first".to_owned())
    );
    assert_eq!(
        from(pointing.bid("dish", "first", &pass(false, 5_000, 30.0))),
        Some("better".to_owned()),
        "once it sets, the best one still up takes over"
    );
}

#[test]
fn a_pass_beats_a_source_without_one_and_takes_over_when_it_rises() {
    let mut pointing = Pointing::default();
    pointing.bid("dish", "sat", &pass(false, 500, 40.0));
    assert_eq!(
        from(pointing.bid("dish", "scan", &passless(true, 60.0))),
        Some("scan".to_owned()),
        "between passes the sky scan sweeps"
    );
    assert_eq!(
        from(pointing.bid("dish", "sat", &pass(true, 500, 40.0))),
        Some("sat".to_owned())
    );
}

#[test]
fn with_none_in_view_the_first_to_rise_leads() {
    let mut pointing = Pointing::default();
    pointing.bid("dish", "later", &pass(false, 500, 80.0));
    assert_eq!(
        from(pointing.bid("dish", "sooner", &pass(false, 200, 15.0))),
        Some("sooner".to_owned())
    );
    assert_eq!(
        from(pointing.bid("dish", "tie", &pass(false, 200, 50.0))),
        Some("tie".to_owned()),
        "rising together, the higher pass leads"
    );
    assert_eq!(
        from(pointing.bid("dish", "never", &passless(false, -5.0))),
        Some("tie".to_owned())
    );
}

#[test]
fn withdrawing_the_lead_hands_the_target_on_and_the_last_leaves_it_free() {
    let mut pointing = Pointing::default();
    pointing.bid("dish", "a", &pass(true, 0, 60.0));
    pointing.bid("dish", "b", &pass(true, 0, 30.0));
    assert_eq!(from(pointing.withdraw("dish", "a")), Some("b".to_owned()));
    assert_eq!(from(pointing.withdraw("dish", "b")), None);
    assert_eq!(from(pointing.withdraw("dish", "b")), None);
}
