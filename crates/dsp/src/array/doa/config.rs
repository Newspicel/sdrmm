use crate::array::manifold::ElevationSpan;

const DEFAULT_MAX_PEAKS: u8 = 2;
const DEFAULT_LOADING: f32 = 1e-3;
const DEFAULT_AZIMUTH_STEP_DEG: f64 = 1.0;
const DEFAULT_SQUELCH_OPEN_DB: f32 = 6.0;
const DEFAULT_SQUELCH_HYSTERESIS_DB: f32 = 1.0;
const DEFAULT_PEAK_RANGE_DB: f32 = 20.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Estimator {
    Bartlett,
    Capon,
    #[default]
    Music,
    RootMusic,
    Esprit,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SourceCount {
    #[default]
    Auto,
    Fixed(u8),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OrderRule {
    #[default]
    Dominance,
    Mdl,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UlaSide {
    #[default]
    Both,
    Front,
    Back,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SquelchConfig {
    pub open_db: f32,
    pub hysteresis_db: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DoaConfig {
    pub estimator: Estimator,
    pub sources: SourceCount,
    pub rule: OrderRule,
    pub max_peaks: u8,
    pub forward_backward: bool,
    pub smoothing: u8,
    pub loading: f32,
    pub azimuth_step_deg: f64,
    pub elevation: Option<ElevationSpan>,
    pub ula_side: UlaSide,
    pub squelch: Option<SquelchConfig>,
    pub peak_range_db: f32,
}

impl Default for DoaConfig {
    fn default() -> Self {
        Self {
            estimator: Estimator::Music,
            sources: SourceCount::Auto,
            rule: OrderRule::Dominance,
            max_peaks: DEFAULT_MAX_PEAKS,
            forward_backward: false,
            smoothing: 0,
            loading: DEFAULT_LOADING,
            azimuth_step_deg: DEFAULT_AZIMUTH_STEP_DEG,
            elevation: None,
            ula_side: UlaSide::Both,
            squelch: Some(SquelchConfig {
                open_db: DEFAULT_SQUELCH_OPEN_DB,
                hysteresis_db: DEFAULT_SQUELCH_HYSTERESIS_DB,
            }),
            peak_range_db: DEFAULT_PEAK_RANGE_DB,
        }
    }
}
