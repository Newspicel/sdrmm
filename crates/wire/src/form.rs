use serde_json::Value;

use crate::channel::{Control, NumberSpec, ParamOption, SettingField};
use crate::fusion::{NavMode, TriangulationParams};
use crate::labelled;
use crate::limits::{BandLimits, Bounds, FUSION_LIMITS};
use crate::processor::df::DfAlgorithm;

pub const MORE: &str = "More";
const KHZ: f64 = 1_000.0;
const BAND_STEP_HZ: f64 = 100.0;

labelled! {
    DfAlgorithm {
        Bartlett => "Bartlett": "Plain beam scan, robust",
        Capon => "Capon": "Sharper beam scan",
        Music => "MUSIC": "Subspace scan, splits close sources",
        RootMusic => "Root-MUSIC": "Grid-free MUSIC, needs a line or circle",
        Esprit => "ESPRIT": "Grid-free rotation fit, needs a line or circle",
    }
    NavMode {
        Auto => "Auto": "Cross the bearings first, then drive at the fix",
        Direct => "Direct": "Drive at the fix as soon as there is one",
        Off => "off",
    }
}

/// A number control spanning `bounds`; a bound that excludes its minimum starts one step up.
pub fn within<T: Copy + Into<f64>>(bounds: Bounds<T>, step: f64) -> NumberSpec {
    let min = decimal(bounds.min.into());
    let min = if bounds.above {
        decimal(min + step)
    } else {
        min
    };
    NumberSpec::new(min, decimal(bounds.max.into()), step)
}

/// `value` to seven significant digits, so an `f32` bound reads as the decimal it was written
/// as and survives a trip through JSON unchanged.
fn decimal(value: f64) -> f64 {
    format!("{value:.6e}").parse().unwrap_or(value)
}

pub fn khz(bounds: Bounds<f64>, step_hz: f64) -> NumberSpec {
    within(bounds, step_hz).scaled(KHZ, "kHz")
}

pub fn choice(options: Vec<ParamOption>) -> Control {
    Control::Choice { options }
}

#[must_use]
pub fn powers_of_two(bounds: Bounds<u32>) -> Control {
    let values: Vec<u32> = (0..u32::BITS)
        .map(|shift| 1_u32 << shift)
        .filter(|value| bounds.contains(*value))
        .collect();
    Control::values(&values, |value| value.to_string())
}

#[must_use]
pub fn offset(band: BandLimits) -> SettingField {
    SettingField::new("offset_hz", "Offset", khz(band.offset_hz, BAND_STEP_HZ))
        .title("From the array centre")
        .quiet(0.0)
}

#[must_use]
pub fn width(band: BandLimits, title: &str) -> SettingField {
    SettingField::new(
        "bandwidth_hz",
        "Width",
        khz(band.bandwidth_hz, BAND_STEP_HZ),
    )
    .title(title)
}

/// A width that may be unset to take every lane sample.
#[must_use]
pub fn full_width(band: BandLimits) -> SettingField {
    SettingField::new(
        "bandwidth_hz",
        "Width",
        khz(band.bandwidth_hz, BAND_STEP_HZ).optional("full"),
    )
    .title("Band around the offset, empty for every lane sample")
    .quiet(Value::Null)
}

#[must_use]
pub fn milliseconds<T: Copy + Into<f64>>(
    key: &str,
    label: &str,
    bounds: Bounds<T>,
    step: f64,
) -> SettingField {
    SettingField::new(key, label, within(bounds, step).unit("ms"))
}

fn half_life(seconds: u32) -> String {
    if seconds.is_multiple_of(60) {
        format!("Half life {} min", seconds / 60)
    } else {
        format!("Half life {seconds} s")
    }
}

impl TriangulationParams {
    #[must_use]
    pub fn form() -> Vec<SettingField> {
        let limits = FUSION_LIMITS;
        vec![
            SettingField::new(
                "decay.kind",
                "Fade",
                choice(vec![
                    ParamOption::new("auto", "Auto")
                        .title("Fixed or moving, from the stations")
                        .status("fade"),
                    ParamOption::new("fixed", "Fixed").title(&half_life(limits.fixed_half_life_s)),
                    ParamOption::new("moving", "Moving")
                        .title(&half_life(limits.moving_half_life_s)),
                    ParamOption::new("half_life", "Set").title("Pick the half life"),
                ]),
            )
            .title("How fast old bearings count less"),
            SettingField::new(
                "decay.seconds",
                "Half life",
                within(limits.half_life_s, 10.0).unit("s"),
            )
            .title("Old bearings count half after this")
            .fallback(limits.half_life_seed_s)
            .when("decay.kind", ["half_life"]),
            SettingField::new("nav", "Guide", Control::choice::<NavMode>())
                .title("Where to send a wired vehicle")
                .quiet(NavMode::Off),
            SettingField::new(
                "extent_km",
                "Extent",
                within(limits.extent_km, 0.1).unit("km"),
            )
            .title("Half width of the search grid"),
            SettingField::new("probe_km", "Probe", within(limits.probe_km, 0.5).unit("km"))
                .title("How far across a single bearing to drive"),
            SettingField::new(
                "min_confidence",
                "Min conf",
                within(limits.min_confidence, 0.01),
            )
            .title("Bearings below this are refused"),
            SettingField::new("align", "Align", Control::Toggle)
                .title("Learn each station's heading offset from crossing bearings"),
            SettingField::new("max_emitters", "Emitters", within(limits.emitters, 1.0))
                .title("Most transmitters to find at once"),
        ]
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod audit;
#[cfg(test)]
mod tests;
