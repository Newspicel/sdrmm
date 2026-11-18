use super::*;
use sdrmm_wire::array::MAX_ARRAY_LANES;

use crate::Settings;

fn with(change: impl FnOnce(&mut PassiveRadarParams)) -> PassiveRadarParams {
    let mut params = PassiveRadarParams::default();
    change(&mut params);
    params
}

#[test]
fn radar_defaults_are_valid() {
    let params = PassiveRadarParams::default();
    assert_eq!(params.problem(), None);
    assert_eq!(
        serde_json::from_str::<PassiveRadarParams>("{}").expect("empty body"),
        params
    );
}

#[test]
fn every_rule_names_its_problem() {
    let cases: Vec<(PassiveRadarParams, &str)> = vec![
        (
            with(|p| p.cpi_ms = RADAR_MIN_CPI_MS - 1),
            "CPI out of range",
        ),
        (
            with(|p| p.cpi_ms = RADAR_MAX_CPI_MS + 1),
            "CPI out of range",
        ),
        (with(|p| p.overlap = 0.8), "Overlap out of range"),
        (with(|p| p.max_range_km = 0.0), "Range out of range"),
        (with(|p| p.max_range_km = 401.0), "Range out of range"),
        (with(|p| p.max_speed_mps = 9.0), "Speed out of range"),
        (with(|p| p.offset_hz = f64::NAN), "Offset out of range"),
        (with(|p| p.offset_hz = 60e6), "Offset out of range"),
        (
            with(|p| {
                p.illuminator = Illuminator::Custom { bandwidth_hz: 5e3 };
            }),
            "Bandwidth out of range",
        ),
        (
            with(|p| p.reference_element = MAX_ARRAY_LANES),
            "Reference out of range",
        ),
        (
            with(|p| p.surveillance = SurveillanceSet::Mask { mask: 0 }),
            "No surveillance element",
        ),
        (
            with(|p| p.surveillance = SurveillanceSet::Mask { mask: 1 << 16 }),
            "No surveillance element",
        ),
        (
            with(|p| p.surveillance = SurveillanceSet::Mask { mask: 0b11 }),
            "Surveillance holds the reference",
        ),
        (with(|p| p.illuminator = Illuminator::Dab), "CMA needs FM"),
        (
            with(|p| p.reference = ReferenceCleaning::DabRemod),
            "DAB remod needs DAB",
        ),
        (
            with(|p| {
                p.reference = ReferenceCleaning::Cma {
                    taps: 0,
                    step: 1e-3,
                }
            }),
            "CMA taps out of range",
        ),
        (
            with(|p| {
                p.reference = ReferenceCleaning::Cma {
                    taps: 16,
                    step: 0.0,
                }
            }),
            "CMA step out of range",
        ),
        (
            with(|p| p.clutter.reach_km = 0.05),
            "Clutter reach out of range",
        ),
        (
            with(|p| p.clutter.lead = MAX_ECA_LEAD + 1),
            "Lead out of range",
        ),
        (
            with(|p| p.clutter.doppler_taps = MAX_ECA_DOPPLER_TAPS + 1),
            "Doppler taps out of range",
        ),
        (with(|p| p.clutter.batch_ms = 0.5), "Batch out of range"),
        (
            with(|p| p.clutter.extension_ms = 501.0),
            "Extension out of range",
        ),
        (with(|p| p.clutter.step = 0.0), "Step out of range"),
        (with(|p| p.clutter.loading = 1.5), "Loading out of range"),
        (
            with(|p| p.cfar.kind = CfarKind::Os { rank: 0.4 }),
            "Rank out of range",
        ),
        (with(|p| p.cfar.pfa = 0.5), "Pfa out of range"),
        (with(|p| p.cfar.guard_range = 17), "Guard out of range"),
        (with(|p| p.cfar.guard_doppler = 17), "Guard out of range"),
        (with(|p| p.cfar.train_range = 0), "Train out of range"),
        (with(|p| p.cfar.train_doppler = 17), "Train out of range"),
        (
            with(|p| p.cfar.min_doppler_hz = 101.0),
            "Min Doppler out of range",
        ),
        (
            with(|p| p.cfar.min_range_km = 51.0),
            "Min range out of range",
        ),
        (with(|p| p.cfar.min_snr_db = -1.0), "Min SNR out of range"),
        (with(|p| p.tracker.confirm_hits = 0), "M of N out of range"),
        (
            with(|p| {
                p.tracker.confirm_hits = 6;
                p.tracker.confirm_window = 5;
            }),
            "M of N out of range",
        ),
        (
            with(|p| p.tracker.confirm_window = 17),
            "M of N out of range",
        ),
        (with(|p| p.tracker.coast_looks = 101), "Coast out of range"),
        (
            with(|p| p.tracker.max_accel_mps2 = 0.5),
            "Accel out of range",
        ),
        (with(|p| p.tracker.gate = 31.0), "Gate out of range"),
        (with(|p| p.tracker.jerk = 0.0), "Jerk out of range"),
        (
            with(|p| p.assumed_altitude_m = 15_001.0),
            "Altitude out of range",
        ),
    ];
    for (params, text) in cases {
        assert_eq!(params.problem(), Some(text), "{params:?}");
        assert!(params.problem().is_some());
    }
}

#[test]
fn dab_and_dvbt_illuminators_are_valid_with_their_cleaning() {
    let dab = with(|p| {
        p.illuminator = Illuminator::Dab;
        p.reference = ReferenceCleaning::DabRemod;
    });
    assert!(dab.problem().is_none(), "{:?}", dab.problem());
    let dvbt = with(|p| {
        p.illuminator = Illuminator::DvbtPartial {
            bandwidth_hz: DVBT_BANDWIDTH_HZ,
        };
        p.reference = ReferenceCleaning::Off;
    });
    assert!(dvbt.problem().is_none(), "{:?}", dvbt.problem());
    let custom_cma = with(|p| p.illuminator = Illuminator::Custom { bandwidth_hz: 1e6 });
    assert!(custom_cma.problem().is_none());
}

#[test]
fn illuminators_name_their_bandwidth() {
    assert_eq!(Illuminator::Fm.bandwidth_hz(), FM_BANDWIDTH_HZ);
    assert_eq!(Illuminator::Dab.bandwidth_hz(), DAB_BANDWIDTH_HZ);
    assert_eq!(
        Illuminator::DvbtPartial { bandwidth_hz: 1e6 }.bandwidth_hz(),
        1e6
    );
    let labels: Vec<_> = [
        Illuminator::Fm,
        Illuminator::Dab,
        Illuminator::DvbtPartial { bandwidth_hz: 1e6 },
        Illuminator::Custom { bandwidth_hz: 1e6 },
    ]
    .iter()
    .map(Illuminator::label)
    .collect();
    assert_eq!(labels, ["FM", "DAB", "DVB-T", "Custom"]);
}

#[test]
fn surveillance_all_others_skips_the_reference() {
    let all: Vec<u32> = SurveillanceSet::AllOthers.elements(2, 5).collect();
    assert_eq!(all, [0, 1, 3, 4]);
    let mask: Vec<u32> = SurveillanceSet::Mask { mask: 0b1_0110 }
        .elements(1, 4)
        .collect();
    assert_eq!(mask, [2]);
}

#[test]
fn max_eca_order_matches_the_solver_limit() {
    assert_eq!(MAX_ECA_ORDER, 512);
}

#[test]
fn the_first_broken_rule_is_named() {
    let cases = [
        (
            with(|p| {
                p.cpi_ms = 0;
                p.assumed_altitude_m = -1.0;
            }),
            "CPI out of range",
        ),
        (
            with(|p| {
                p.reference_element = MAX_ARRAY_LANES;
                p.surveillance = SurveillanceSet::Mask { mask: 0 };
            }),
            "Reference out of range",
        ),
        (
            with(|p| {
                p.surveillance = SurveillanceSet::Mask { mask: 0b1 };
                p.illuminator = Illuminator::Dab;
            }),
            "Surveillance holds the reference",
        ),
        (
            with(|p| {
                p.illuminator = Illuminator::Dab;
                p.clutter.reach_km = 0.0;
            }),
            "CMA needs FM",
        ),
        (
            with(|p| {
                p.clutter.loading = 2.0;
                p.cfar.pfa = 1.0;
            }),
            "Loading out of range",
        ),
        (
            with(|p| {
                p.cfar.min_snr_db = 41.0;
                p.tracker.gate = 0.0;
            }),
            "Min SNR out of range",
        ),
        (
            with(|p| {
                p.tracker.jerk = f32::NAN;
                p.assumed_altitude_m = f32::NAN;
            }),
            "Jerk out of range",
        ),
    ];
    for (params, text) in cases {
        assert_eq!(params.problem(), Some(text), "{params:?}");
    }
}
