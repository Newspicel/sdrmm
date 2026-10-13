use super::*;
use crate::{
    LatLon, WsprSpot,
    decode::Symbol,
    fusion::{DfBearing, DfEstimate, RAY_LENGTH_M},
    radar::{RadarTrackEvent, TrackChange},
};

fn at(lat: f64, lon: f64) -> LatLon {
    LatLon { lat, lon }
}

fn spot_of(text: &str) -> Option<Spot> {
    ft8(text).present().spot
}

fn wspr(grid: Option<&str>) -> DecoderEvent {
    DecoderEvent::Wspr(WsprSpot {
        text: "K1ABC FN42 37".to_owned(),
        callsign: "K1ABC".to_owned(),
        grid: grid.map(str::to_owned),
        power_dbm: 37,
        snr_db: -24.0,
        audio_hz: 1500.0,
        time_offset_s: 0.0,
        drift_hz: 0.0,
    })
}

#[test]
fn a_square_sits_at_its_own_centre() {
    assert_eq!(grid_lat_lon("FN42"), Some(at(42.5, -71.0)));
    assert_eq!(grid_lat_lon("JO62"), Some(at(52.5, 13.0)));
    assert_eq!(grid_lat_lon("IO91"), Some(at(51.5, -1.0)));
}

#[test]
fn lower_case_and_finer_subsquares_are_read() {
    let coarse = grid_lat_lon("JO62").expect("square");
    let fine = grid_lat_lon("jo62qm").expect("subsquare");
    assert!((fine.lat - coarse.lat).abs() < 0.5);
    assert!((fine.lon - coarse.lon).abs() < 1.0);
    assert!(grid_lat_lon("JO62QM12").is_some());
}

#[test]
fn anything_but_a_locator_is_refused() {
    for bad in [
        "", "FN4", "FN421", "ZZ99", "F142", "FN4X", "JO62yy", "JO62QM1",
    ] {
        assert_eq!(grid_lat_lon(bad), None, "{bad}");
    }
}

#[test]
fn a_cq_or_a_reply_carries_its_square() {
    for (text, grid) in [
        ("CQ W1AW FN42", "FN42"),
        ("CQ DX JA1ABC PM95", "PM95"),
        ("W9XYZ W1AW FN42", "FN42"),
        ("cq w1aw fn42", "FN42"),
    ] {
        let spot = spot_of(text).expect(text);
        assert_eq!(spot.grid, grid, "{text}");
        assert_eq!(Some(spot.at), grid_lat_lon(grid), "{text}");
    }
}

#[test]
fn reports_and_rr73_are_no_square() {
    for text in [
        "W9XYZ W1AW RR73",
        "W9XYZ W1AW RRR",
        "W9XYZ W1AW 73",
        "W9XYZ W1AW -12",
        "W9XYZ W1AW R-12",
        "W9XYZ W1AW R+05",
        "TU; W9XYZ W1AW R 579 MA",
    ] {
        assert_eq!(spot_of(text), None, "{text}");
    }
}

#[test]
fn the_square_names_the_station_before_it() {
    for text in ["CQ W1AW FN42", "W9XYZ W1AW FN42", "CQ <W1AW> FN42"] {
        assert_eq!(spot_of(text).expect(text).callsign, "W1AW", "{text}");
    }
}

#[test]
fn a_wspr_spot_reads_its_own_grid() {
    let spot = wspr(Some("FN42")).present().spot.expect("spot");
    assert_eq!(spot.grid, "FN42");
    assert_eq!(spot.callsign, "K1ABC");
    assert_eq!(spot.snr_db, -24.0);
    assert_eq!(spot.at, at(42.5, -71.0));
    assert_eq!(wspr(None).present().spot, None);
    assert_eq!(wspr(Some("ZZ99")).present().spot, None);
}

#[test]
fn a_placed_bearing_draws_its_ray_and_faint_other_peaks() {
    let view = view(EventKind::Df);
    let ray = view.bearing.expect("bearing");
    assert_eq!(ray.from, at(52.5, 13.4));
    assert_eq!(ray.true_deg, 87.4);
    assert_eq!(ray.sigma_deg, Some(4.6));
    assert_eq!(ray.confidence, 0.83);
    assert_eq!(
        view.shapes,
        vec![
            Shape::Ray {
                from: ray.from,
                bearing_deg: 87.4,
                sigma_deg: Some(4.6),
                length_m: RAY_LENGTH_M,
                faint: false,
            },
            Shape::Ray {
                from: ray.from,
                bearing_deg: 212.0,
                sigma_deg: Some(9.0),
                length_m: RAY_LENGTH_M,
                faint: true,
            },
        ]
    );
    assert_eq!(RAY_LENGTH_M, 25_000.0);
}

#[test]
fn an_unplaced_bearing_draws_nothing() {
    let DecoderEvent::Df(placed) = fixture(EventKind::Df) else {
        panic!("a bearing");
    };
    let unplaced = DfBearing {
        lat: None,
        lon: None,
        ..placed
    };
    let view = unplaced.present(EventKind::Df);
    assert_eq!(view.bearing, None);
    assert!(view.shapes.is_empty());
}

#[test]
fn a_fix_draws_its_ellipse_by_half_axes() {
    assert_eq!(
        view(EventKind::DfFix).shapes,
        vec![Shape::Ellipse {
            center: at(52.51, 13.38),
            major_m: 60.0,
            minor_m: 20.0,
            bearing_deg: 30.0,
        }]
    );
    let DecoderEvent::DfFix(fix) = fixture(EventKind::DfFix) else {
        panic!("a fix");
    };
    let flat = DfEstimate {
        ellipse_minor_m: 0.0,
        ..fix
    };
    assert!(flat.present(EventKind::DfFix).shapes.is_empty());
}

#[test]
fn a_sonde_forecast_draws_its_path_burst_and_landing() {
    assert_eq!(
        view(EventKind::Radiosonde).shapes,
        vec![
            Shape::Line {
                points: vec![at(52.2, 14.1), at(52.4, 14.3), at(52.3, 14.6)],
                style: LineStyle::Forecast,
            },
            Shape::Pin {
                at: at(52.4, 14.3),
                symbol: Symbol::Dot,
                label: "Burst".to_owned(),
            },
            Shape::Pin {
                at: at(52.3, 14.6),
                symbol: Symbol::Target,
                label: "Landing".to_owned(),
            },
        ]
    );
}

#[test]
fn a_sonde_without_a_full_path_keeps_only_its_marks() {
    let DecoderEvent::Radiosonde(mut sonde) = fixture(EventKind::Radiosonde) else {
        panic!("a sonde");
    };
    if let Some(forecast) = sonde.forecast.as_mut() {
        forecast.path.truncate(1);
    }
    let marks = sonde.present(EventKind::Radiosonde).shapes;
    assert_eq!(marks.len(), 2);
    assert!(marks.iter().all(|shape| matches!(shape, Shape::Pin { .. })));
    sonde.forecast = None;
    assert!(sonde.present(EventKind::Radiosonde).shapes.is_empty());
}

#[test]
fn a_lost_radar_track_is_gone() {
    let DecoderEvent::Radar(track) = fixture(EventKind::Radar) else {
        panic!("a track");
    };
    assert!(!track.present(EventKind::Radar).gone);
    let lost = RadarTrackEvent {
        change: TrackChange::Lost,
        ..track
    };
    assert!(lost.present(EventKind::Radar).gone);
}
