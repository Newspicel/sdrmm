use serde_json::Value;

use crate::channel::{Control, LanePick, NumberSpec, ParamOption, SettingField};
use crate::fusion::{NavMode, TriangulationParams};
use crate::labelled;
use crate::limits::{
    BEAMFORMER_LIMITS, BandLimits, Bounds, CORRELATOR_LIMITS, DF_LIMITS, FUSION_LIMITS,
    POLARIMETER_LIMITS, SPATIAL_LIMITS,
};
use crate::processor::beamformer::{Adaptation, BeamMode, BeamformerParams, NoiseModel};
use crate::processor::correlator::CorrelatorParams;
use crate::processor::df::{DfAlgorithm, DfParams, SourceRule, UlaSide};
use crate::processor::polarimeter::PolarimeterParams;
use crate::processor::spatial::{SpatialMethod, SpatialSpectrumParams};
use crate::processor::stitch::{StitchBlend, StitchParams};

mod radar;

pub(crate) const MORE: &str = "More";
const KHZ: f64 = 1_000.0;
const BAND_STEP_HZ: f64 = 100.0;
const FULL_CIRCLE_DEG: u32 = 360;

labelled! {
    DfAlgorithm {
        Bartlett => "Bartlett": "Plain beam scan, robust",
        Capon => "Capon": "Sharper beam scan",
        Music => "MUSIC": "Subspace scan, splits close sources",
        RootMusic => "Root-MUSIC": "Grid-free MUSIC, needs a line or circle",
        Esprit => "ESPRIT": "Grid-free rotation fit, needs a line or circle",
    }
    SourceRule {
        Dominance => "Dominance": "Count sources clearly above the rest",
        Mdl => "MDL": "Information criterion",
    }
    UlaSide {
        Both => "Both": "Report front and mirror",
        Front => "Front": "Only the side ahead of the line",
        Back => "Back": "Only the side behind the line",
    }
    BeamMode {
        Mrc => "MRC": "Best SNR from every lane",
        Das => "Beam": "Steer with equal weights",
        Mvdr => "MVDR": "Steer and suppress the rest",
        Lcmv => "Nulls": "Steer with nulls",
        Gsc => "GSC": "Steer and cancel the rest as it moves",
        Canceller => "Cancel": "Subtract what reference lanes hear",
        Cma => "CMA": "Lock onto a steady envelope",
    }
    Adaptation {
        Nlms => "NLMS": "Light, follows slowly",
        Rls => "RLS": "Heavier, follows fast",
    }
    NoiseModel {
        Measured => "Measured": "Noise of each lane as measured",
        White => "White": "Same noise on every lane",
    }
    StitchBlend {
        Snr => "SNR": "Favour the cleaner lane in overlaps",
        Equal => "Equal": "Average overlaps",
    }
    SpatialMethod {
        Bartlett => "Bartlett": "Robust, wide peaks",
        Capon => "Capon": "Sharper peaks",
        Music => "MUSIC": "Sharpest, needs clean signals",
    }
    NavMode {
        Auto => "Auto": "Cross the bearings first, then drive at the fix",
        Direct => "Direct": "Drive at the fix as soon as there is one",
        Off => "off",
    }
}

/// A number control spanning `bounds`; a bound that excludes its minimum starts one step up.
pub(crate) fn within<T: Copy + Into<f64>>(bounds: Bounds<T>, step: f64) -> NumberSpec {
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

pub(crate) fn khz(bounds: Bounds<f64>, step_hz: f64) -> NumberSpec {
    within(bounds, step_hz).scaled(KHZ, "kHz")
}

pub(crate) fn choice(options: Vec<ParamOption>) -> Control {
    Control::Choice { options }
}

fn powers_of_two(bounds: Bounds<u32>) -> Control {
    let values: Vec<u32> = (0..u32::BITS)
        .map(|shift| 1_u32 << shift)
        .filter(|value| bounds.contains(*value))
        .collect();
    Control::values(&values, |value| value.to_string())
}

fn offset(band: BandLimits) -> SettingField {
    SettingField::new("offset_hz", "Offset", khz(band.offset_hz, BAND_STEP_HZ))
        .title("From the array centre")
        .quiet(0.0)
}

fn width(band: BandLimits, title: &str) -> SettingField {
    SettingField::new(
        "bandwidth_hz",
        "Width",
        khz(band.bandwidth_hz, BAND_STEP_HZ),
    )
    .title(title)
}

/// A width that may be unset to take every lane sample.
fn full_width(band: BandLimits) -> SettingField {
    SettingField::new(
        "bandwidth_hz",
        "Width",
        khz(band.bandwidth_hz, BAND_STEP_HZ).optional("full"),
    )
    .title("Band around the offset, empty for every lane sample")
    .quiet(Value::Null)
}

fn milliseconds<T: Copy + Into<f64>>(
    key: &str,
    label: &str,
    bounds: Bounds<T>,
    step: f64,
) -> SettingField {
    SettingField::new(key, label, within(bounds, step).unit("ms"))
}

impl DfParams {
    #[must_use]
    pub fn form() -> Vec<SettingField> {
        let peaks: Vec<u8> = (DF_LIMITS.peaks.min..=DF_LIMITS.peaks.max).collect();
        let mut form = vec![
            SettingField::new("algorithm", "Method", Control::choice::<DfAlgorithm>()),
            offset(DF_LIMITS.band),
            width(DF_LIMITS.band, "Band the finder listens to"),
            SettingField::new(
                "sources",
                "Sources",
                within(DF_LIMITS.sources, 1.0).optional("Auto"),
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
                within(DF_LIMITS.squelch_db, 0.5).unit("dB"),
            )
            .title("Minimum peak above the floor, 0 off")
            .quiet(0.0),
            milliseconds("report_ms", "Report", DF_LIMITS.report_ms, 100.0),
            SettingField::new("elevation", "Elevation", Control::Toggle)
                .title("Estimate elevation too, needs a planar array"),
            SettingField::new(
                "station_id",
                "Station",
                Control::Text {
                    placeholder: Some("unnamed".to_owned()),
                    optional: true,
                    secret: false,
                    max_len: Some(DF_LIMITS.station_len),
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
            SettingField::new("smoothing", "Smooth", within(DF_LIMITS.smoothing, 1.0))
                .title("Spatial smoothing for reflections, 0 off, needs a line or circle")
                .quiet(0),
            SettingField::new("loading", "Loading", within(DF_LIMITS.loading, 0.001))
                .title("Diagonal loading, steadies Capon and MUSIC"),
            SettingField::new(
                "azimuth_step_deg",
                "Step",
                within(DF_LIMITS.azimuth_step_deg, 0.25).unit("°"),
            )
            .title("Scan grid"),
            SettingField::new(
                "yaw_gate_dps",
                "Yaw gate",
                within(DF_LIMITS.yaw_gate_dps, 1.0).unit("°/s"),
            )
            .title("Skip blocks while the array turns faster"),
            SettingField::new(
                "carry_over",
                "Carry over",
                within(DF_LIMITS.carry_over, 0.01),
            )
            .title("Share of the last report kept"),
        ]
        .into_iter()
        .map(|field| field.section(MORE))
        .collect()
    }
}

const STEERED: [BeamMode; 4] = [BeamMode::Das, BeamMode::Mvdr, BeamMode::Lcmv, BeamMode::Gsc];
const BLOCKED: [BeamMode; 4] = [BeamMode::Mrc, BeamMode::Das, BeamMode::Mvdr, BeamMode::Lcmv];
const COVARIANCE: [BeamMode; 3] = [BeamMode::Mrc, BeamMode::Mvdr, BeamMode::Lcmv];

impl BeamformerParams {
    #[must_use]
    pub fn form() -> Vec<SettingField> {
        let mut form = vec![
            SettingField::new("mode", "Mode", Control::choice::<BeamMode>())
                .title("How lanes combine"),
        ];
        form.extend(Self::steer_form());
        form.extend(Self::null_form());
        form.extend(Self::canceller_form());
        form.extend(Self::solver_form());
        form.extend(Self::block_form());
        form.push(offset(BEAMFORMER_LIMITS.band));
        form.push(full_width(BEAMFORMER_LIMITS.band));
        form
    }

    fn steer_form() -> Vec<SettingField> {
        vec![
            SettingField::new(
                "steer.kind",
                "Steer",
                choice(vec![
                    ParamOption::new("wired", "DF").title("Follow the wired direction finder"),
                    ParamOption::new("fixed", "Fixed").title("One azimuth"),
                ]),
            )
            .title("Beam direction")
            .when("mode", STEERED),
            SettingField::new(
                "steer.azimuth_deg",
                "Azimuth",
                NumberSpec::new(0.0, 359.9, 0.1).unit("°"),
            )
            .title("From array forward")
            .fallback(0.0)
            .when("mode", STEERED)
            .when("steer.kind", ["fixed"]),
            milliseconds(
                "steer_timeout_ms",
                "Timeout",
                BEAMFORMER_LIMITS.steer_timeout_ms,
                100.0,
            )
            .title("Hold the last bearing this long")
            .when("mode", STEERED)
            .when("steer.kind", ["wired"]),
        ]
    }

    fn null_form() -> Vec<SettingField> {
        let nulled = [BeamMode::Lcmv, BeamMode::Gsc];
        vec![
            SettingField::new(
                "nulls_deg",
                "Nulls",
                Control::numbers(
                    NumberSpec::new(0.0, 359.9, 0.1).unit("°"),
                    BEAMFORMER_LIMITS.nulls,
                ),
            )
            .title("Azimuths to silence")
            .quiet(Vec::<f64>::new())
            .when("mode", nulled),
            SettingField::new("auto_nulls", "Auto nulls", Control::Toggle)
                .title("Find and silence strong interferers")
                .when("mode", nulled),
            SettingField::new(
                "loading",
                "Loading",
                within(BEAMFORMER_LIMITS.loading, 0.01),
            )
            .title("Diagonal loading, steadies the solve")
            .when("mode", [BeamMode::Mvdr, BeamMode::Lcmv, BeamMode::Gsc]),
            SettingField::new("noise", "Noise", Control::choice::<NoiseModel>())
                .title("Noise model")
                .when("mode", [BeamMode::Mrc]),
        ]
    }

    fn canceller_form() -> Vec<SettingField> {
        let canceller = [BeamMode::Canceller];
        vec![
            SettingField::new("main_lane", "Main", Control::lane())
                .title("Lane the others are cancelled from")
                .when("mode", canceller),
            SettingField::new(
                "reference_lanes",
                "Refs",
                Control::lanes_except(LanePick::List, "main_lane").or_empty("others"),
            )
            .title("Lanes that hear only the interference, none for every other lane")
            .quiet(Vec::<u32>::new())
            .when("mode", canceller),
            SettingField::new("taps", "Taps", within(BEAMFORMER_LIMITS.taps, 1.0))
                .title("Delays per reference lane")
                .when("mode", canceller),
        ]
    }

    fn solver_form() -> Vec<SettingField> {
        let canceller = [BeamMode::Canceller];
        let tapped = 2..=BEAMFORMER_LIMITS.taps.max;
        let step = || {
            SettingField::new("step", "Step", within(BEAMFORMER_LIMITS.step, 0.000_01))
                .title("How fast weights follow")
        };
        vec![
            SettingField::new("adaptation", "Adapt", Control::choice::<Adaptation>())
                .title("Weight solver")
                .when("mode", canceller)
                .when("taps", tapped.clone()),
            step()
                .when("mode", canceller)
                .when("taps", tapped.clone())
                .when("adaptation", [Adaptation::Nlms]),
            step().when("mode", [BeamMode::Gsc, BeamMode::Cma]),
            SettingField::new(
                "forget",
                "Forget",
                within(BEAMFORMER_LIMITS.forget, 0.000_01),
            )
            .title("Memory of the RLS solver")
            .when("mode", canceller)
            .when("taps", tapped)
            .when("adaptation", [Adaptation::Rls]),
        ]
    }

    /// The block settings show for the block modes, and for a canceller of one tap.
    fn block_form() -> Vec<SettingField> {
        let fields = || {
            [
                milliseconds("update_ms", "Update", BEAMFORMER_LIMITS.update_ms, 10.0)
                    .title("New weights this often"),
                milliseconds("crossfade_ms", "Fade", BEAMFORMER_LIMITS.crossfade_ms, 1.0)
                    .title("Blend old and new weights"),
            ]
        };
        let carry = || {
            SettingField::new(
                "carry_over",
                "Carry over",
                within(BEAMFORMER_LIMITS.carry_over, 0.01),
            )
            .title("Share of the last estimate kept")
        };
        let mut form: Vec<SettingField> = fields()
            .into_iter()
            .map(|field| field.when("mode", BLOCKED))
            .collect();
        form.push(carry().when("mode", COVARIANCE));
        form.extend(
            fields()
                .into_iter()
                .chain([carry()])
                .map(|field| field.when("mode", [BeamMode::Canceller]).when("taps", [1])),
        );
        form
    }
}

impl StitchParams {
    #[must_use]
    pub fn form() -> Vec<SettingField> {
        vec![
            SettingField::new("blend", "Blend", Control::choice::<StitchBlend>()),
            SettingField::new("noise_equalise", "Equalise", Control::Toggle)
                .title("Match noise floors"),
            SettingField::new("flatten", "Flatten", Control::Toggle)
                .title("Undo each lane's filter droop"),
            SettingField::new("spur_reject", "Spurs", Control::Toggle)
                .title("Drop lane spurs in overlaps"),
            SettingField::new("match_phase", "Phase", Control::Toggle)
                .title("Line up phase across seams"),
        ]
    }
}

impl SpatialSpectrumParams {
    #[must_use]
    pub fn form() -> Vec<SettingField> {
        let limits = SPATIAL_LIMITS;
        let steps: Vec<f64> = (1..=FULL_CIRCLE_DEG)
            .filter(|deg| FULL_CIRCLE_DEG.is_multiple_of(*deg))
            .map(f64::from)
            .filter(|deg| limits.azimuth_step_deg.contains(*deg))
            .collect();
        vec![
            SettingField::new("method", "Method", Control::choice::<SpatialMethod>()),
            SettingField::new("bins", "FFT", powers_of_two(limits.bins)).title("Frequency bins"),
            SettingField::new("columns", "Columns", powers_of_two(limits.columns))
                .title("Frequency columns shown")
                .at_most("bins"),
            milliseconds("average_ms", "Average", limits.average_ms, 10.0),
            milliseconds("report_ms", "Rate", limits.report_ms, 10.0)
                .title("New picture this often"),
            SettingField::new(
                "azimuth_step_deg",
                "Step",
                Control::values(&steps, |deg| format!("{deg}°")),
            )
            .title("Bearing resolution"),
            SettingField::new("span_db", "Range", within(limits.span_db, 1.0).unit("dB"))
                .title("Colour range below the peak"),
            offset(limits.band),
            full_width(limits.band),
        ]
    }
}

impl CorrelatorParams {
    #[must_use]
    pub fn form() -> Vec<SettingField> {
        let limits = CORRELATOR_LIMITS;
        vec![
            SettingField::new("bins", "FFT", powers_of_two(limits.bins)).title("Frequency bins"),
            SettingField::new("channels", "Channels", powers_of_two(limits.channels))
                .title("Frequency points sent per baseline")
                .at_most("bins"),
            SettingField::new(
                "integrate_s",
                "Integrate",
                within(limits.integrate_s, 0.05).unit("s"),
            )
            .title("Averaging time per result"),
            SettingField::new("overlap", "Overlap", Control::Toggle)
                .title("Half-overlapping FFTs, twice the work"),
            offset(limits.band),
            full_width(limits.band),
        ]
    }
}

impl PolarimeterParams {
    #[must_use]
    pub fn form() -> Vec<SettingField> {
        let limits = POLARIMETER_LIMITS;
        vec![
            SettingField::new(
                "h_lane",
                "H",
                Control::lanes_except(LanePick::One, "v_lane"),
            )
            .title("Lane of the horizontal antenna"),
            SettingField::new(
                "v_lane",
                "V",
                Control::lanes_except(LanePick::One, "h_lane"),
            )
            .title("Lane of the vertical antenna"),
            SettingField::new(
                "matched",
                "Output",
                choice(vec![
                    ParamOption::new(true, "Matched").title("Beam output matched to the wave"),
                    ParamOption::new(false, "Cross").title("Beam output across the wave"),
                ]),
            )
            .title("What the beam output carries"),
            offset(limits.band),
            width(limits.band, "Band around the offset"),
            milliseconds("report_ms", "Report", limits.report_ms, 10.0),
            milliseconds("average_ms", "Average", limits.average_ms, 10.0),
            milliseconds("crossfade_ms", "Fade", limits.crossfade_ms, 1.0)
                .title("Blend old and new weights"),
            SettingField::new("flip_hand", "Flip", Control::Toggle)
                .title("Swap right and left hand"),
        ]
    }
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

#[cfg(test)]
mod tests;
