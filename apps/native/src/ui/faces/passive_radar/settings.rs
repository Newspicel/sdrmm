use sdrmm_wire::{
    array::MAX_ARRAY_LANES,
    limits::{Bounds as Limit, RADAR_LIMITS as LIMITS},
    patch::NodeBody,
    radar::{
        CfarKind, CfarWindow, ClutterMethod, DopplerWindow, GpuUse, Illuminator,
        PassiveRadarParams, ReferenceCleaning, SurveillanceSet,
    },
};

use crate::ui::{faces::scope::colormap::COLORMAPS, kit_raster::Bounds};

const PFA_EXPONENTS: std::ops::RangeInclusive<i32> = 3..=8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IlluminatorKind {
    Fm,
    Dab,
    Dvbt,
    Custom,
}

pub const ILLUMINATOR_OPTIONS: [(IlluminatorKind, &str); 4] = [
    (IlluminatorKind::Fm, "FM"),
    (IlluminatorKind::Dab, "DAB"),
    (IlluminatorKind::Dvbt, "DVB-T"),
    (IlluminatorKind::Custom, "Custom"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleaningKind {
    Off,
    Cma,
    DabRemod,
}

pub const CLEANING_OPTIONS: [(CleaningKind, &str); 3] = [
    (CleaningKind::Off, "Off"),
    (CleaningKind::Cma, "CMA"),
    (CleaningKind::DabRemod, "DAB remod"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CfarChoice {
    Ca,
    Os,
    Go,
}

pub const CFAR_OPTIONS: [(CfarChoice, &str); 3] = [
    (CfarChoice::Ca, "CA"),
    (CfarChoice::Os, "OS"),
    (CfarChoice::Go, "GO"),
];

pub const CFAR_WINDOW_OPTIONS: [(CfarWindow, &str); 2] =
    [(CfarWindow::Range, "Range"), (CfarWindow::Plane, "2D")];

pub const CLUTTER_OPTIONS: [(ClutterMethod, &str); 5] = [
    (ClutterMethod::EcaBatch, "ECA-B"),
    (ClutterMethod::EcaSliding, "ECA-S"),
    (ClutterMethod::Nlms, "NLMS"),
    (ClutterMethod::BlockNlms, "Block NLMS"),
    (ClutterMethod::Off, "Off"),
];

pub const WINDOW_OPTIONS: [(DopplerWindow, &str); 3] = [
    (DopplerWindow::Hann, "Hann"),
    (DopplerWindow::BlackmanHarris, "Blackman-Harris"),
    (DopplerWindow::Rectangular, "None"),
];

pub const GPU_OPTIONS: [(GpuUse, &str); 2] = [(GpuUse::Auto, "Auto"), (GpuUse::Off, "Off")];

#[must_use]
pub fn options<T: Copy>(choices: &[(T, &str)]) -> Vec<(T, String)> {
    choices
        .iter()
        .map(|(value, label)| (*value, (*label).to_owned()))
        .collect()
}

#[must_use]
pub fn colour_options() -> Vec<(u8, String)> {
    COLORMAPS
        .iter()
        .enumerate()
        .map(|(index, map)| (index as u8, capitalised(map.name())))
        .collect()
}

fn capitalised(name: &str) -> String {
    let mut letters = name.chars();
    letters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(letters).collect()
    })
}

#[must_use]
pub fn params_of(body: &NodeBody) -> Option<&PassiveRadarParams> {
    match body {
        NodeBody::PassiveRadar(radar) => Some(&radar.settings),
        _ => None,
    }
}

pub fn params_mut(body: &mut NodeBody) -> Option<&mut PassiveRadarParams> {
    match body {
        NodeBody::PassiveRadar(radar) => Some(&mut radar.settings),
        _ => None,
    }
}

#[must_use]
pub fn span<T: Copy + PartialOrd + Into<f64>>(limit: Limit<T>, scale: f64, step: f64) -> Bounds {
    let min = limit.min.into() * scale;
    Bounds::new(
        if limit.above { min + step } else { min },
        limit.max.into() * scale,
    )
}

#[must_use]
pub fn whole<T: Copy + PartialOrd + Into<f64>>(limit: Limit<T>) -> Bounds {
    Bounds::whole(limit.min.into(), limit.max.into())
}

#[must_use]
pub const fn kind_of(illuminator: &Illuminator) -> IlluminatorKind {
    match illuminator {
        Illuminator::Fm => IlluminatorKind::Fm,
        Illuminator::Dab => IlluminatorKind::Dab,
        Illuminator::DvbtPartial { .. } => IlluminatorKind::Dvbt,
        Illuminator::Custom { .. } => IlluminatorKind::Custom,
    }
}

#[must_use]
pub const fn bandwidth_of(illuminator: &Illuminator) -> Option<f64> {
    match illuminator {
        Illuminator::DvbtPartial { bandwidth_hz } | Illuminator::Custom { bandwidth_hz } => {
            Some(*bandwidth_hz)
        }
        Illuminator::Fm | Illuminator::Dab => None,
    }
}

#[must_use]
pub const fn with_bandwidth(illuminator: Illuminator, bandwidth_hz: f64) -> Illuminator {
    match illuminator {
        Illuminator::DvbtPartial { .. } => Illuminator::DvbtPartial { bandwidth_hz },
        Illuminator::Custom { .. } => Illuminator::Custom { bandwidth_hz },
        Illuminator::Fm | Illuminator::Dab => illuminator,
    }
}

#[must_use]
pub fn illuminator_of(kind: IlluminatorKind, previous: Illuminator) -> Illuminator {
    if kind == kind_of(&previous) {
        return previous;
    }
    let carried = bandwidth_of(&previous);
    match kind {
        IlluminatorKind::Fm => Illuminator::Fm,
        IlluminatorKind::Dab => Illuminator::Dab,
        IlluminatorKind::Dvbt => Illuminator::DvbtPartial {
            bandwidth_hz: carried.unwrap_or(LIMITS.seed.dvbt_bandwidth_hz),
        },
        IlluminatorKind::Custom => Illuminator::Custom {
            bandwidth_hz: carried.unwrap_or(LIMITS.seed.custom_bandwidth_hz),
        },
    }
}

#[must_use]
pub const fn cleaning_kind(cleaning: &ReferenceCleaning) -> CleaningKind {
    match cleaning {
        ReferenceCleaning::Off => CleaningKind::Off,
        ReferenceCleaning::Cma { .. } => CleaningKind::Cma,
        ReferenceCleaning::DabRemod => CleaningKind::DabRemod,
    }
}

#[must_use]
pub const fn cleaning_fits(kind: CleaningKind, illuminator: IlluminatorKind) -> bool {
    match kind {
        CleaningKind::Off => true,
        CleaningKind::Cma => matches!(illuminator, IlluminatorKind::Fm | IlluminatorKind::Custom),
        CleaningKind::DabRemod => matches!(illuminator, IlluminatorKind::Dab),
    }
}

#[must_use]
pub fn cleaning_options(illuminator: IlluminatorKind) -> Vec<(CleaningKind, String)> {
    CLEANING_OPTIONS
        .iter()
        .filter(|(kind, _)| cleaning_fits(*kind, illuminator))
        .map(|(kind, label)| (*kind, (*label).to_owned()))
        .collect()
}

#[must_use]
pub fn cleaning_of(kind: CleaningKind, previous: ReferenceCleaning) -> ReferenceCleaning {
    if kind == cleaning_kind(&previous) {
        return previous;
    }
    match kind {
        CleaningKind::Off => ReferenceCleaning::Off,
        CleaningKind::Cma => ReferenceCleaning::Cma {
            taps: LIMITS.seed.cma_taps,
            step: LIMITS.seed.cma_step,
        },
        CleaningKind::DabRemod => ReferenceCleaning::DabRemod,
    }
}

pub fn switch_illuminator(params: &mut PassiveRadarParams, kind: IlluminatorKind) {
    params.illuminator = illuminator_of(kind, params.illuminator);
    if !cleaning_fits(cleaning_kind(&params.reference), kind) {
        params.reference = ReferenceCleaning::Off;
    }
}

#[must_use]
pub const fn cfar_choice(kind: &CfarKind) -> CfarChoice {
    match kind {
        CfarKind::Ca => CfarChoice::Ca,
        CfarKind::Os { .. } => CfarChoice::Os,
        CfarKind::Go => CfarChoice::Go,
    }
}

#[must_use]
pub fn cfar_kind_of(choice: CfarChoice, previous: CfarKind) -> CfarKind {
    if choice == cfar_choice(&previous) {
        return previous;
    }
    match choice {
        CfarChoice::Ca => CfarKind::Ca,
        CfarChoice::Os => CfarKind::Os {
            rank: LIMITS.seed.os_rank,
        },
        CfarChoice::Go => CfarKind::Go,
    }
}

#[must_use]
pub fn pfa_options() -> Vec<(i32, String)> {
    PFA_EXPONENTS
        .map(|exponent| (exponent, format!("1e-{exponent}")))
        .collect()
}

#[must_use]
pub fn pfa_exponent(pfa: f64) -> i32 {
    let exponent = if pfa > 0.0 {
        (-pfa.log10()).round() as i32
    } else {
        *PFA_EXPONENTS.end()
    };
    exponent.clamp(*PFA_EXPONENTS.start(), *PFA_EXPONENTS.end())
}

#[must_use]
pub fn pfa_of(exponent: i32) -> f64 {
    10f64.powi(-exponent)
}

fn bit(element: u32) -> u32 {
    1u32.checked_shl(element).unwrap_or(0)
}

#[must_use]
pub fn all_elements(lanes: u32) -> Vec<u32> {
    (0..lanes.min(MAX_ARRAY_LANES)).collect()
}

#[must_use]
pub fn surveillance_of(set: SurveillanceSet, reference: u32, lanes: u32) -> Vec<u32> {
    all_elements(lanes)
        .into_iter()
        .filter(|element| match set {
            SurveillanceSet::AllOthers => *element != reference,
            SurveillanceSet::Mask { mask } => mask & bit(*element) != 0,
        })
        .collect()
}

#[must_use]
pub fn toggled_surveillance(
    set: SurveillanceSet,
    element: u32,
    reference: u32,
    lanes: u32,
) -> SurveillanceSet {
    if element == reference {
        return set;
    }
    let mut current = surveillance_of(set, reference, lanes);
    match current.iter().position(|held| *held == element) {
        Some(at) => {
            current.remove(at);
        }
        None => current.push(element),
    }
    if current.is_empty() {
        return set;
    }
    let everyone = all_elements(lanes)
        .into_iter()
        .filter(|index| *index != reference)
        .all(|index| current.contains(&index));
    if everyone {
        return SurveillanceSet::AllOthers;
    }
    SurveillanceSet::Mask {
        mask: current.iter().fold(0, |mask, index| mask | bit(*index)),
    }
}

pub fn set_reference(params: &mut PassiveRadarParams, reference: u32) {
    params.reference_element = reference;
    if let SurveillanceSet::Mask { mask } = params.surveillance
        && mask & bit(reference) != 0
    {
        let left = mask & !bit(reference);
        params.surveillance = if left == 0 {
            SurveillanceSet::AllOthers
        } else {
            SurveillanceSet::Mask { mask: left }
        };
    }
}

#[must_use]
pub fn element_options(lanes: u32) -> Vec<(u32, String)> {
    all_elements(lanes.max(1))
        .into_iter()
        .map(|element| (element, format!("Element {}", element + 1)))
        .collect()
}

#[must_use]
pub const fn eca(method: ClutterMethod) -> bool {
    matches!(method, ClutterMethod::EcaBatch | ClutterMethod::EcaSliding)
}

#[must_use]
pub const fn nlms(method: ClutterMethod) -> bool {
    matches!(method, ClutterMethod::Nlms | ClutterMethod::BlockNlms)
}

pub fn cma_taps(params: &mut PassiveRadarParams, taps: f64) {
    if let ReferenceCleaning::Cma { taps: held, .. } = &mut params.reference {
        *held = taps as u32;
    }
}

pub fn cma_step(params: &mut PassiveRadarParams, step: f64) {
    if let ReferenceCleaning::Cma { step: held, .. } = &mut params.reference {
        *held = step as f32;
    }
}

pub fn cma_read(params: &PassiveRadarParams) -> (f64, f64) {
    match params.reference {
        ReferenceCleaning::Cma { taps, step } => (f64::from(taps), f64::from(step)),
        ReferenceCleaning::Off | ReferenceCleaning::DabRemod => (0.0, 0.0),
    }
}

pub fn rank_read(params: &PassiveRadarParams) -> f64 {
    match params.cfar.kind {
        CfarKind::Os { rank } => f64::from(rank * 100.0).round(),
        CfarKind::Ca | CfarKind::Go => 0.0,
    }
}

pub fn rank_write(params: &mut PassiveRadarParams, percent: f64) {
    params.cfar.kind = CfarKind::Os {
        rank: (percent / 100.0) as f32,
    };
}

pub fn start_hits(params: &mut PassiveRadarParams, hits: f64) {
    params.tracker.confirm_hits = (hits as u32).min(params.tracker.confirm_window);
}

pub fn start_looks(params: &mut PassiveRadarParams, looks: f64) {
    params.tracker.confirm_window = (looks as u32).max(params.tracker.confirm_hits);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn radar() -> PassiveRadarParams {
        PassiveRadarParams::default()
    }

    #[test]
    fn switching_to_dvbt_seeds_its_bandwidth_and_custom_carries_it() {
        let dvbt = illuminator_of(IlluminatorKind::Dvbt, Illuminator::Fm);
        assert_eq!(
            dvbt,
            Illuminator::DvbtPartial {
                bandwidth_hz: LIMITS.seed.dvbt_bandwidth_hz
            }
        );
        assert_eq!(
            illuminator_of(IlluminatorKind::Custom, dvbt),
            Illuminator::Custom {
                bandwidth_hz: LIMITS.seed.dvbt_bandwidth_hz
            }
        );
        assert_eq!(
            illuminator_of(IlluminatorKind::Custom, Illuminator::Dab),
            Illuminator::Custom {
                bandwidth_hz: LIMITS.seed.custom_bandwidth_hz
            }
        );
        assert_eq!(illuminator_of(IlluminatorKind::Fm, dvbt), Illuminator::Fm);
    }

    #[test]
    fn picking_the_same_illuminator_keeps_its_bandwidth() {
        let custom = Illuminator::Custom {
            bandwidth_hz: 123_000.0,
        };
        assert_eq!(illuminator_of(IlluminatorKind::Custom, custom), custom);
    }

    #[test]
    fn a_bandwidth_only_lands_on_illuminators_that_carry_one() {
        assert_eq!(bandwidth_of(&Illuminator::Fm), None);
        let custom = with_bandwidth(Illuminator::Custom { bandwidth_hz: 1.0 }, 5e5);
        assert_eq!(bandwidth_of(&custom), Some(5e5));
        assert_eq!(with_bandwidth(Illuminator::Dab, 5e5), Illuminator::Dab);
    }

    #[test]
    fn switching_illuminator_drops_cleaning_that_no_longer_fits() {
        let mut params = radar();
        switch_illuminator(&mut params, IlluminatorKind::Dab);
        assert_eq!(params.illuminator, Illuminator::Dab);
        assert_eq!(params.reference, ReferenceCleaning::Off);
        params.reference = ReferenceCleaning::DabRemod;
        switch_illuminator(&mut params, IlluminatorKind::Dab);
        assert_eq!(params.reference, ReferenceCleaning::DabRemod);
        let mut fm = radar();
        let kept = fm.reference;
        switch_illuminator(&mut fm, IlluminatorKind::Custom);
        assert_eq!(fm.reference, kept);
    }

    #[test]
    fn only_fitting_cleaning_is_offered() {
        let labels = |kind| {
            cleaning_options(kind)
                .into_iter()
                .map(|(_, label)| label)
                .collect::<Vec<_>>()
        };
        assert_eq!(labels(IlluminatorKind::Fm), vec!["Off", "CMA"]);
        assert_eq!(labels(IlluminatorKind::Dab), vec!["Off", "DAB remod"]);
        assert_eq!(labels(IlluminatorKind::Dvbt), vec!["Off"]);
    }

    #[test]
    fn cma_cleaning_starts_from_the_seed() {
        assert_eq!(
            cleaning_of(CleaningKind::Cma, ReferenceCleaning::Off),
            ReferenceCleaning::Cma {
                taps: LIMITS.seed.cma_taps,
                step: LIMITS.seed.cma_step
            }
        );
        let tuned = ReferenceCleaning::Cma {
            taps: 7,
            step: 0.01,
        };
        assert_eq!(cleaning_of(CleaningKind::Cma, tuned), tuned);
    }

    #[test]
    fn os_cfar_starts_from_the_seed_rank() {
        assert_eq!(
            cfar_kind_of(CfarChoice::Os, CfarKind::Ca),
            CfarKind::Os {
                rank: LIMITS.seed.os_rank
            }
        );
        let ranked = CfarKind::Os { rank: 0.6 };
        assert_eq!(cfar_kind_of(CfarChoice::Os, ranked), ranked);
        assert_eq!(cfar_kind_of(CfarChoice::Go, ranked), CfarKind::Go);
    }

    #[test]
    fn pfa_snaps_to_the_nearest_offered_power() {
        assert_eq!(pfa_exponent(1e-5), 5);
        assert_eq!(pfa_exponent(3e-6), 6);
        assert_eq!(pfa_exponent(1e-2), 3);
        assert_eq!(pfa_exponent(1e-12), 8);
        assert_eq!(pfa_exponent(0.0), 8);
        assert!((pfa_of(4) - 1e-4).abs() < 1e-18);
        assert_eq!(
            pfa_options().first().map(|(_, label)| label.as_str()),
            Some("1e-3")
        );
    }

    #[test]
    fn all_others_listens_on_every_element_but_the_reference() {
        assert_eq!(
            surveillance_of(SurveillanceSet::AllOthers, 1, 4),
            vec![0, 2, 3]
        );
        assert_eq!(
            surveillance_of(SurveillanceSet::Mask { mask: 0b1010 }, 0, 4),
            vec![1, 3]
        );
    }

    #[test]
    fn toggling_surveillance_builds_a_mask_and_folds_back_to_all() {
        let set = toggled_surveillance(SurveillanceSet::AllOthers, 2, 0, 4);
        assert_eq!(set, SurveillanceSet::Mask { mask: 0b1010 });
        assert_eq!(
            toggled_surveillance(set, 2, 0, 4),
            SurveillanceSet::AllOthers
        );
        assert_eq!(toggled_surveillance(set, 0, 0, 4), set);
        let lone = SurveillanceSet::Mask { mask: 0b10 };
        assert_eq!(toggled_surveillance(lone, 1, 0, 4), lone);
    }

    #[test]
    fn a_new_reference_leaves_the_surveillance_set() {
        let mut params = PassiveRadarParams {
            surveillance: SurveillanceSet::Mask { mask: 0b110 },
            ..radar()
        };
        set_reference(&mut params, 2);
        assert_eq!(params.reference_element, 2);
        assert_eq!(params.surveillance, SurveillanceSet::Mask { mask: 0b10 });
        set_reference(&mut params, 1);
        assert_eq!(params.surveillance, SurveillanceSet::AllOthers);
    }

    #[test]
    fn elements_are_counted_from_one() {
        assert_eq!(element_options(0), vec![(0, "Element 1".to_owned())]);
        assert_eq!(element_options(3).len(), 3);
        assert_eq!(all_elements(40).len(), MAX_ARRAY_LANES as usize);
    }

    #[test]
    fn exclusive_limits_start_one_step_above_their_floor() {
        let jerk = span(LIMITS.jerk, 1.0, 0.1);
        assert!((jerk.min - 0.1).abs() < 1e-9);
        let offset = span(LIMITS.offset_hz, 1e-3, 1.0);
        assert!((offset.max - 50_000.0).abs() < 1e-9);
        assert!(whole(LIMITS.cpi_ms).whole);
    }

    #[test]
    fn colours_read_capitalised() {
        assert_eq!(
            colour_options().first().map(|(_, label)| label.as_str()),
            Some("Classic")
        );
    }

    #[test]
    fn the_node_body_yields_radar_params_only_for_a_radar() {
        assert!(params_of(&NodeBody::PassiveRadar(Default::default())).is_some());
        assert!(params_of(&NodeBody::Scope).is_none());
    }

    #[test]
    fn the_start_window_never_drops_below_its_hits() {
        let mut params = PassiveRadarParams::default();
        start_hits(&mut params, 9.0);
        assert_eq!(params.tracker.confirm_hits, params.tracker.confirm_window);
        start_looks(&mut params, 1.0);
        assert_eq!(params.tracker.confirm_window, params.tracker.confirm_hits);
    }

    #[test]
    fn cma_fields_only_touch_cma_cleaning() {
        let mut params = PassiveRadarParams::default();
        cma_taps(&mut params, 8.0);
        cma_step(&mut params, 0.01);
        assert_eq!(cma_read(&params), (8.0, f64::from(0.01f32)));
        params.reference = ReferenceCleaning::Off;
        cma_taps(&mut params, 4.0);
        assert_eq!(params.reference, ReferenceCleaning::Off);
        assert_eq!(cma_read(&params), (0.0, 0.0));
    }

    #[test]
    fn the_os_rank_reads_and_writes_in_percent() {
        let mut params = PassiveRadarParams::default();
        assert_eq!(rank_read(&params), 0.0);
        rank_write(&mut params, 80.0);
        assert_eq!(rank_read(&params), 80.0);
    }
}
