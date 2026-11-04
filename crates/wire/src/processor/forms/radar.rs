use super::{choice, khz, milliseconds, within};
use crate::channel::{Control, LanePick, ParamOption, SettingField};
use crate::labelled;
use crate::limits::{Bounds, RADAR_LIMITS as LIMITS};
use crate::radar::{CfarWindow, ClutterMethod, DopplerWindow, GpuUse, PassiveRadarParams};

const CLUTTER: &str = "Clutter";
const DETECT: &str = "Detect";
const TRACK: &str = "Track";
const KHZ_STEP: f64 = 1_000.0;
const PFA_EXPONENTS: std::ops::RangeInclusive<i32> = 3..=8;

labelled! {
    ClutterMethod {
        EcaBatch => "ECA-B": "Batched extensive cancellation",
        EcaSliding => "ECA-S": "Sliding extensive cancellation",
        Nlms => "NLMS",
        BlockNlms => "Block NLMS",
        Off => "off",
    }
    CfarWindow {
        Range => "Range": "Across delay per Doppler row",
        Plane => "2D": "Across delay and Doppler",
    }
    DopplerWindow {
        Hann => "Hann",
        BlackmanHarris => "Blackman-Harris",
        Rectangular => "none",
    }
    GpuUse {
        Auto => "Auto": "GPU when it is faster",
        Off => "off",
    }
}

const ECA: [ClutterMethod; 2] = [ClutterMethod::EcaBatch, ClutterMethod::EcaSliding];
const ADAPTIVE: [ClutterMethod; 2] = [ClutterMethod::Nlms, ClutterMethod::BlockNlms];
const CLUTTER_ON: [ClutterMethod; 4] = [
    ClutterMethod::EcaBatch,
    ClutterMethod::EcaSliding,
    ClutterMethod::Nlms,
    ClutterMethod::BlockNlms,
];

impl PassiveRadarParams {
    #[must_use]
    pub fn form() -> Vec<SettingField> {
        let mut form = illuminator_form();
        form.extend(element_form());
        form.extend(cleaning_form());
        form.extend(search_form());
        form.extend(clutter_form());
        form.extend(detect_form());
        form.extend(track_form());
        form
    }
}

fn illuminator_form() -> Vec<SettingField> {
    let bandwidth = |kind: &str, seed: f64| {
        SettingField::new(
            "illuminator.bandwidth_hz",
            "Bandwidth",
            khz(LIMITS.bandwidth_hz, KHZ_STEP),
        )
        .title("Illuminator bandwidth")
        .fallback(seed)
        .when("illuminator.kind", [kind])
    };
    vec![
        SettingField::new(
            "illuminator.kind",
            "Illum",
            choice(vec![
                ParamOption::new("fm", "FM"),
                ParamOption::new("dab", "DAB"),
                ParamOption::new("dvbt_partial", "DVB-T"),
                ParamOption::new("custom", "Custom"),
            ]),
        )
        .title("Transmitter type"),
        bandwidth("dvbt_partial", LIMITS.seed.dvbt_bandwidth_hz),
        bandwidth("custom", LIMITS.seed.custom_bandwidth_hz),
        SettingField::new("offset_hz", "Offset", khz(LIMITS.offset_hz, KHZ_STEP))
            .title("Illuminator minus array centre")
            .quiet(0.0),
    ]
}

fn element_form() -> Vec<SettingField> {
    vec![
        SettingField::new("reference_element", "Ref", Control::lane())
            .title("Element pointed at the transmitter"),
        SettingField::new(
            "surveillance.kind",
            "Listen",
            choice(vec![
                ParamOption::new("all_others", "Others").title("Every element but the reference"),
                ParamOption::new("mask", "Pick").title("Chosen elements"),
            ]),
        )
        .title("Elements that listen for echoes"),
        SettingField::new(
            "surveillance.mask",
            "Elements",
            Control::lanes_except(LanePick::Mask, "reference_element"),
        )
        .title("Elements that listen for echoes")
        .fallback(2)
        .when("surveillance.kind", ["mask"]),
    ]
}

/// Cleaning fits the illuminator: CMA needs FM or Custom, remodulation needs DAB.
fn cleaning_form() -> Vec<SettingField> {
    let off = || ParamOption::new("off", "off");
    let cleaning = |options: Vec<ParamOption>, kinds: &[&str]| {
        SettingField::new("reference.kind", "Clean", choice(options))
            .title("Reference cleaning")
            .quiet("off")
            .when("illuminator.kind", kinds.iter().copied())
    };
    vec![
        cleaning(
            vec![off(), ParamOption::new("cma", "CMA")],
            &["fm", "custom"],
        ),
        cleaning(
            vec![off(), ParamOption::new("dab_remod", "DAB remod")],
            &["dab"],
        ),
        cleaning(vec![off()], &["dvbt_partial"]),
        SettingField::new("reference.taps", "CMA taps", within(LIMITS.cma_taps, 1.0))
            .fallback(LIMITS.seed.cma_taps)
            .when("reference.kind", ["cma"]),
        SettingField::new(
            "reference.step",
            "CMA step",
            within(LIMITS.cma_step, 0.000_1),
        )
        .fallback(LIMITS.seed.cma_step)
        .when("reference.kind", ["cma"]),
    ]
}

fn search_form() -> Vec<SettingField> {
    let pfa: Vec<ParamOption> = PFA_EXPONENTS
        .map(|exponent| ParamOption::new(10_f64.powi(-exponent), &format!("1e-{exponent}")))
        .collect();
    vec![
        SettingField::new(
            "max_range_km",
            "Range",
            within(LIMITS.max_range_km, 1.0).unit("km"),
        )
        .title("Bistatic excess range searched"),
        SettingField::new(
            "max_speed_mps",
            "Speed",
            within(LIMITS.max_speed_mps, 10.0).unit("m/s"),
        )
        .title("Largest range rate searched"),
        milliseconds("cpi_ms", "CPI", LIMITS.cpi_ms, 10.0).title("Coherent processing interval"),
        SettingField::new(
            "overlap",
            "Overlap",
            within(LIMITS.overlap, 0.05).scaled(0.01, "%"),
        )
        .title("Share of each CPI reused by the next")
        .quiet(0.0),
        SettingField::new("aoa", "AoA", Control::Toggle)
            .title("Angle of arrival per echo, needs calibration"),
        SettingField::new("gpu", "GPU", Control::choice::<GpuUse>())
            .title("Where the heavy maths runs"),
        SettingField::new(
            "clutter.method",
            "Clutter",
            Control::choice::<ClutterMethod>(),
        )
        .title("Clutter method")
        .quiet(ClutterMethod::Off),
        SettingField::new("cfar.pfa", "Pfa", choice(pfa)).title("False alarm chance per cell"),
    ]
}

fn clutter_form() -> Vec<SettingField> {
    [
        SettingField::new(
            "clutter.reach_km",
            "Reach",
            within(LIMITS.reach_km, 0.1).unit("km"),
        )
        .title("Clutter extent removed")
        .when("clutter.method", CLUTTER_ON),
        SettingField::new("clutter.lead", "Lead", within(LIMITS.lead, 1.0))
            .title("Taps before zero delay")
            .when("clutter.method", CLUTTER_ON),
        SettingField::new(
            "clutter.doppler_taps",
            "Doppler taps",
            within(LIMITS.doppler_taps, 1.0),
        )
        .title("Doppler shifts each side")
        .when("clutter.method", ECA),
        milliseconds("clutter.batch_ms", "Batch", LIMITS.batch_ms, 1.0)
            .when("clutter.method", [ClutterMethod::EcaBatch]),
        SettingField::new("clutter.taper", "Taper", Control::Toggle)
            .title("Blend weights between batches")
            .when("clutter.method", [ClutterMethod::EcaBatch]),
        milliseconds(
            "clutter.extension_ms",
            "Extension",
            LIMITS.extension_ms,
            1.0,
        )
        .when("clutter.method", [ClutterMethod::EcaSliding]),
        SettingField::new(
            "clutter.loading",
            "Loading",
            within(LIMITS.loading, 0.000_1),
        )
        .title("Diagonal loading")
        .when("clutter.method", ECA),
        SettingField::new("clutter.step", "Step", within(LIMITS.clutter_step, 0.001))
            .when("clutter.method", ADAPTIVE),
    ]
    .into_iter()
    .map(|field| field.section(CLUTTER))
    .collect()
}

fn detect_form() -> Vec<SettingField> {
    let plane = [CfarWindow::Plane];
    [
        SettingField::new(
            "cfar.kind.kind",
            "CFAR",
            choice(vec![
                ParamOption::new("ca", "CA").title("Cell averaging"),
                ParamOption::new("os", "OS").title("Ordered statistic"),
                ParamOption::new("go", "GO").title("Greatest of"),
            ]),
        ),
        SettingField::new(
            "cfar.kind.rank",
            "Rank",
            within(LIMITS.os_rank, 0.01).scaled(0.01, "%"),
        )
        .title("Ordered cell used as the noise estimate")
        .fallback(LIMITS.seed.os_rank)
        .when("cfar.kind.kind", ["os"]),
        SettingField::new("cfar.window", "Window", Control::choice::<CfarWindow>())
            .title("Cells the noise is taken from"),
        cells("cfar.guard_range", "Guard", LIMITS.guard)
            .title("Cells skipped next to the cell tested"),
        cells("cfar.train_range", "Train", LIMITS.train_range)
            .title("Cells averaged for the noise"),
        cells("cfar.guard_doppler", "Guard Doppler", LIMITS.guard).when("cfar.window", plane),
        cells("cfar.train_doppler", "Train Doppler", LIMITS.train_doppler)
            .when("cfar.window", plane),
        SettingField::new(
            "cfar.min_doppler_hz",
            "Min Doppler",
            within(LIMITS.min_doppler_hz, 0.5).unit("Hz"),
        )
        .title("Slower rows count as clutter"),
        SettingField::new(
            "cfar.min_range_km",
            "Min range",
            within(LIMITS.min_range_km, 0.5).unit("km"),
        ),
        SettingField::new(
            "cfar.min_snr_db",
            "Min SNR",
            within(LIMITS.min_snr_db, 0.5).unit("dB"),
        ),
        SettingField::new(
            "window",
            "Doppler window",
            Control::choice::<DopplerWindow>(),
        )
        .title("Taper across batches"),
    ]
    .into_iter()
    .map(|field| field.section(DETECT))
    .collect()
}

fn cells(key: &str, label: &str, bounds: Bounds<u32>) -> SettingField {
    SettingField::new(key, label, within(bounds, 1.0))
}

fn track_form() -> Vec<SettingField> {
    [
        SettingField::new(
            "tracker.confirm_hits",
            "Start hits",
            within(LIMITS.track_window, 1.0),
        )
        .title("Hits needed to start a track")
        .at_most("tracker.confirm_window"),
        SettingField::new(
            "tracker.confirm_window",
            "Start looks",
            within(LIMITS.track_window, 1.0),
        )
        .title("Looks the start hits are counted over"),
        SettingField::new(
            "tracker.coast_looks",
            "Coast",
            within(LIMITS.coast_looks, 1.0),
        )
        .title("Missed looks before a track ends"),
        SettingField::new(
            "tracker.max_accel_mps2",
            "Max accel",
            within(LIMITS.max_accel_mps2, 1.0).unit("m/s²"),
        ),
        SettingField::new("tracker.gate", "Gate", within(LIMITS.gate, 0.1))
            .title("How far an echo may land from its track"),
        SettingField::new("tracker.jerk", "Jerk", within(LIMITS.jerk, 0.1))
            .title("How fast targets may change acceleration"),
        SettingField::new(
            "assumed_altitude_m",
            "Altitude",
            within(LIMITS.altitude_m, 100.0).unit("m"),
        )
        .title("Target height when no truth matches"),
    ]
    .into_iter()
    .map(|field| field.section(TRACK))
    .collect()
}
