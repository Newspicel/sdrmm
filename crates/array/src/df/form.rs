use sdrmm_wire::channel::{Control, SettingField};
use sdrmm_wire::form::{MORE, milliseconds, offset, width, within};
use sdrmm_wire::labelled;
use serde_json::Value;

use super::{DfParams, LIMITS, SourceRule, UlaSide};
use sdrmm_wire::DfAlgorithm;

labelled! {
    SourceRule {
        Dominance => "Dominance": "Count sources clearly above the rest",
        Mdl => "MDL": "Information criterion",
    }
    UlaSide {
        Both => "Both": "Report front and mirror",
        Front => "Front": "Only the side ahead of the line",
        Back => "Back": "Only the side behind the line",
    }
}

impl DfParams {
    #[must_use]
    pub fn form() -> Vec<SettingField> {
        let peaks: Vec<u8> = (LIMITS.peaks.min..=LIMITS.peaks.max).collect();
        let mut form = vec![
            SettingField::new("algorithm", "Method", Control::choice::<DfAlgorithm>()),
            offset(LIMITS.band),
            width(LIMITS.band, "Band the finder listens to"),
            SettingField::new(
                "sources",
                "Sources",
                within(LIMITS.sources, 1.0).optional("Auto"),
            )
            .title("Sources, Auto counts them from the signal")
            .quiet(Value::Null),
            SettingField::new("source_rule", "Rule", Control::choice::<SourceRule>())
                .title("How Auto counts sources")
                .when("sources", [Value::Null]),
            SettingField::new(
                "max_peaks",
                "Peaks",
                Control::values(&peaks, |count| count.to_string()),
            )
            .title("Most bearings per report"),
            SettingField::new(
                "squelch_db",
                "Squelch",
                within(LIMITS.squelch_db, 0.5).unit("dB"),
            )
            .title("Minimum peak above the floor, 0 off")
            .quiet(0.0),
            milliseconds("report_ms", "Report", LIMITS.report_ms, 100.0),
            SettingField::new("elevation", "Elevation", Control::Toggle)
                .title("Estimate elevation too, needs a planar array"),
            SettingField::new(
                "station_id",
                "Station",
                Control::Text {
                    placeholder: Some("unnamed".to_owned()),
                    optional: true,
                    secret: false,
                    max_len: Some(LIMITS.station_len),
                },
            )
            .title("Name on the bearings this finder sends")
            .quiet(Value::Null),
        ];
        form.extend(Self::more_form());
        form
    }

    fn more_form() -> Vec<SettingField> {
        [
            SettingField::new("ula_side", "Side", Control::choice::<UlaSide>())
                .title("Which side of a line array to report"),
            SettingField::new("forward_backward", "FB", Control::Toggle)
                .title("Forward-backward averaging, needs a symmetric array"),
            SettingField::new("smoothing", "Smooth", within(LIMITS.smoothing, 1.0))
                .title("Spatial smoothing for reflections, 0 off, needs a line or circle")
                .quiet(0),
            SettingField::new("loading", "Loading", within(LIMITS.loading, 0.001))
                .title("Diagonal loading, steadies Capon and MUSIC"),
            SettingField::new(
                "azimuth_step_deg",
                "Step",
                within(LIMITS.azimuth_step_deg, 0.25).unit("°"),
            )
            .title("Scan grid"),
            SettingField::new(
                "yaw_gate_dps",
                "Yaw gate",
                within(LIMITS.yaw_gate_dps, 1.0).unit("°/s"),
            )
            .title("Skip blocks while the array turns faster"),
            SettingField::new("carry_over", "Carry over", within(LIMITS.carry_over, 0.01))
                .title("Share of the last report kept"),
        ]
        .into_iter()
        .map(|field| field.section(MORE))
        .collect()
    }
}
