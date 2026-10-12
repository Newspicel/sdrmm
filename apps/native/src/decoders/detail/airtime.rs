use std::borrow::Cow;

use sdrmm_wire::{
    airtime::{IsmSurveyReport, WifiBand, WifiOccupancyReport},
    units::hertz,
};

use super::{EventDetail, Row, detail, some};
use crate::decoders::text::{join, share};

#[must_use]
pub const fn band_label(band: WifiBand) -> &'static str {
    match band {
        WifiBand::Ghz2_4 => "2.4 GHz",
        WifiBand::Ghz5 => "5 GHz",
        WifiBand::Ghz6 => "6 GHz",
    }
}

fn dbfs(value: Option<f32>, prefix: &str) -> String {
    value.map_or_else(String::new, |v| format!("{prefix}{v:.0} dBFS"))
}

pub fn wifi_occupancy(report: &WifiOccupancyReport) -> EventDetail {
    let mut rows: Vec<Row> = report
        .channels
        .iter()
        .map(|channel| {
            let label = format!("{} ch {}", band_label(channel.band), channel.number);
            let value = join([
                format!("{} busy", share(channel.busy)),
                dbfs(channel.level_dbfs, ""),
                dbfs(channel.floor_dbfs, "floor "),
            ]);
            (Cow::Owned(label), Some(value))
        })
        .collect();
    rows.push(("Measured".into(), some(share(report.measured))));
    detail(rows, None)
}

fn centres(mhz: &[f64]) -> String {
    let listed: Vec<String> = mhz.iter().map(f64::to_string).collect();
    listed.join(", ")
}

pub fn ism_survey(report: &IsmSurveyReport) -> EventDetail {
    let mut rows: Vec<Row> = vec![
        (
            "Window".into(),
            some(format!(
                "{} to {}",
                hertz(report.low_hz),
                hertz(report.high_hz)
            )),
        ),
        ("Busy".into(), some(share(report.busy))),
    ];
    rows.extend(report.kinds.iter().map(|load| {
        let value = format!(
            "{} bursts · {} · {} µs · {} MHz",
            load.bursts,
            share(load.airtime),
            load.mean_us.round(),
            centres(&load.centres_mhz)
        );
        (Cow::Borrowed(load.kind.label()), Some(value))
    }));
    rows.extend([
        (
            "Floor".into(),
            report.floor_dbfs.map(|v| format!("{v:.0} dBFS")),
        ),
        ("Measured".into(), some(share(report.measured))),
        (
            "Dropped".into(),
            (report.dropped > 0).then(|| report.dropped.to_string()),
        ),
    ]);
    detail(rows, None)
}
