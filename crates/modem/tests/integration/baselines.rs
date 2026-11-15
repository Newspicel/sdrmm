#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use sdrmm_modem_test_support::ber::{Curve, limits, sweep};

/// Bit error rate a committed curve's sensitivity is read at.
const SENSITIVITY_BER: f64 = 1e-3;

pub fn baseline_path(stem: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("baselines/{stem}.json"))
}

pub fn load_curve(stem: &str) -> Curve {
    sweep::load_json(&baseline_path(stem)).unwrap()
}

pub fn sensitivity(stem: &str) -> f64 {
    limits::ebn0_at_ber(&load_curve(stem), SENSITIVITY_BER)
        .expect("committed curve must bracket BER 1e-3")
}
