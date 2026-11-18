use sdrmm_wire::channel::{Control, LanePick, NumberSpec, ParamOption, SettingField};
use sdrmm_wire::form::{choice, full_width, milliseconds, offset, within};
use sdrmm_wire::labelled;

use super::{Adaptation, BeamMode, BeamformerParams, LIMITS, NoiseModel};

labelled! {
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
        form.push(offset(LIMITS.band));
        form.push(full_width(LIMITS.band));
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
                LIMITS.steer_timeout_ms,
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
                Control::numbers(NumberSpec::new(0.0, 359.9, 0.1).unit("°"), LIMITS.nulls),
            )
            .title("Azimuths to silence")
            .quiet(Vec::<f64>::new())
            .when("mode", nulled),
            SettingField::new("auto_nulls", "Auto nulls", Control::Toggle)
                .title("Find and silence strong interferers")
                .when("mode", nulled),
            SettingField::new("loading", "Loading", within(LIMITS.loading, 0.01))
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
            SettingField::new("taps", "Taps", within(LIMITS.taps, 1.0))
                .title("Delays per reference lane")
                .when("mode", canceller),
        ]
    }

    fn solver_form() -> Vec<SettingField> {
        let canceller = [BeamMode::Canceller];
        let tapped = 2..=LIMITS.taps.max;
        let step = || {
            SettingField::new("step", "Step", within(LIMITS.step, 0.000_01))
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
            SettingField::new("forget", "Forget", within(LIMITS.forget, 0.000_01))
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
                milliseconds("update_ms", "Update", LIMITS.update_ms, 10.0)
                    .title("New weights this often"),
                milliseconds("crossfade_ms", "Fade", LIMITS.crossfade_ms, 1.0)
                    .title("Blend old and new weights"),
            ]
        };
        let carry = || {
            SettingField::new("carry_over", "Carry over", within(LIMITS.carry_over, 0.01))
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
