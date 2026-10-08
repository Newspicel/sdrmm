#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use sdrmm_modem_test_support::ber::{
    Curve,
    catalog::{
        FULL_ERRORS,
        ble::{
            self, CODED_S2_AWGN, CODED_S2_GRID, CODED_S2_SEED, CODED_S8_AWGN, CODED_S8_GRID,
            CODED_S8_SEED, LE1M_AWGN, LE1M_GRID, LE1M_SEED,
        },
        wifi::{self, DSSS_AWGN, DSSS_GRID, DSSS_SEED, OFDM_AWGN, OFDM_GRID, OFDM_SEED},
    },
    impair::ChannelSpec,
    limits,
    sweep::{self, Link},
};

type Row = (
    &'static str,
    fn() -> Link,
    &'static [f64],
    u64,
    &'static str,
    u64,
);

const ROWS: &[Row] = &[
    (
        "ble 1m",
        ble::le1m_link,
        LE1M_GRID,
        LE1M_SEED,
        LE1M_AWGN,
        ble::FULL_CAP,
    ),
    (
        "ble coded s2",
        ble::coded_s2_link,
        CODED_S2_GRID,
        CODED_S2_SEED,
        CODED_S2_AWGN,
        ble::FULL_CAP,
    ),
    (
        "ble coded s8",
        ble::coded_s8_link,
        CODED_S8_GRID,
        CODED_S8_SEED,
        CODED_S8_AWGN,
        ble::FULL_CAP,
    ),
    (
        "wifi dsss 1m",
        wifi::dsss_link,
        DSSS_GRID,
        DSSS_SEED,
        DSSS_AWGN,
        wifi::FULL_CAP,
    ),
    (
        "wifi ofdm 6m",
        wifi::ofdm_link,
        OFDM_GRID,
        OFDM_SEED,
        OFDM_AWGN,
        wifi::FULL_CAP,
    ),
];

fn baseline_path(stem: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("baselines/{stem}.json"))
}

fn load_curve(stem: &str) -> Curve {
    sweep::load_json(&baseline_path(stem)).unwrap()
}

fn sensitivity(stem: &str) -> f64 {
    limits::ebn0_at_ber(&load_curve(stem), 1e-3).expect("committed curve must bracket BER 1e-3")
}

#[test]
fn every_packet_link_round_trips_clean_at_high_ebn0() {
    for (name, link, ..) in ROWS {
        let ber = limits::measure_ber(&link(), &ChannelSpec::default(), 20.0, 0x0_9a11, 1, 1);
        assert!(ber == 0.0, "{name} floor {ber} at 20 dB Eb/N0");
    }
}

#[test]
fn every_committed_packet_curve_matches_its_baseline() {
    for (name, link, grid, seed, stem, cap) in ROWS {
        let committed = load_curve(stem);
        let measured = sweep::sweep_ber(
            &link(),
            &ChannelSpec::default(),
            &grid[..3],
            *seed,
            FULL_ERRORS,
            *cap,
        );
        let worst = sweep::worst_penalty_db_vs_curve(&measured, &committed, grid[0], grid[2]);
        assert!(worst.abs() < 0.5, "{name} drift vs committed: {worst} dB");
    }
}

fn snr_at_sensitivity(link: fn() -> Link, stem: &str) -> f64 {
    let link = link();
    let bits = vec![true; link.bits_per_trial];
    let energy: f64 = (link.modulate)(&bits)
        .iter()
        .map(|x| f64::from(x.norm_sqr()))
        .sum();
    sensitivity(stem) - 10.0 * (energy / link.bits_per_trial as f64).log10()
}

#[test]
fn coding_buys_long_range_its_margin() {
    let one = snr_at_sensitivity(ble::le1m_link, LE1M_AWGN);
    let s2 = snr_at_sensitivity(ble::coded_s2_link, CODED_S2_AWGN);
    let s8 = snr_at_sensitivity(ble::coded_s8_link, CODED_S8_AWGN);
    assert!(s2 < one - 3.0, "S2 {s2} dB vs 1M {one} dB");
    assert!(s8 < s2 - 2.0, "S8 {s8} dB vs S2 {s2} dB");
}

#[test]
#[ignore = "full sweep; run in release to (re)generate the committed curves"]
fn measure_all_packet_curves_full() {
    for (name, link, grid, seed, stem, cap) in ROWS {
        let curve = sweep::sweep_ber(
            &link(),
            &ChannelSpec::default(),
            grid,
            *seed,
            FULL_ERRORS,
            *cap,
        );
        for point in &curve.points {
            println!("{name} {:5.1} dB  BER {:.3e}", point.ebn0_db, point.rate());
        }
        let path = baseline_path(stem);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        sweep::save_json(&curve, &path).unwrap();
    }
}
