use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub const MAX_SCAN_TARGETS: usize = 20_000;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ScanRange {
    pub start_hz: f64,
    pub stop_hz: f64,
    pub step_hz: f64,
}

/// What a scan is looking for. Every mode calls a frequency busy once it stands `margin_db` over
/// the noise floor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScanMode {
    /// Steps a list of frequencies and holds on the first busy one until it goes quiet.
    #[default]
    Targets,
    /// Watches the whole span for the loudest carrier and holds on that, wherever it turns out to
    /// be.
    CloseCall,
    All,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WatchedLane {
    pub device_set: u32,
    pub stream: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ScanSettings {
    #[serde(skip)]
    pub channel: u32,
    #[serde(skip)]
    pub lane: Option<WatchedLane>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    #[serde(default)]
    pub mode: ScanMode,
    #[serde(default)]
    pub ranges: Vec<ScanRange>,
    #[serde(default)]
    pub frequencies: Vec<f64>,
    /// Frequencies the scan steps over without ever holding on them.
    #[serde(default, alias = "skip", skip_serializing_if = "Vec::is_empty")]
    pub lockouts: Vec<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub priority: Vec<f64>,
    #[serde(default = "default_dwell_ms")]
    pub dwell_ms: u32,
    #[serde(default = "default_resume_ms")]
    pub resume_ms: u32,
    #[serde(default = "default_hold_ms")]
    pub hold_ms: u32,
    /// The slice measured around each target. Left out, the decoder's own bandwidth is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measure_bw_hz: Option<f64>,
    /// Whether to let a radio that sweeps in its own firmware do the sweeping. Radios without one
    /// retune for every step either way.
    #[serde(default = "default_hardware_sweep")]
    pub hardware_sweep: bool,
    /// How far over the noise floor a frequency has to stand to count as busy.
    #[serde(default = "default_margin_db")]
    pub margin_db: f32,
}

pub const PRIORITY_INTERVAL_MS: u32 = 2_000;

fn default_dwell_ms() -> u32 {
    250
}
fn default_resume_ms() -> u32 {
    1_500
}
fn default_hold_ms() -> u32 {
    5_000
}
const fn default_hardware_sweep() -> bool {
    true
}
fn default_margin_db() -> f32 {
    12.0
}

impl ScanSettings {
    #[must_use]
    pub fn for_channel(channel: u32) -> Self {
        Self {
            channel,
            lane: None,
            node: None,
            mode: ScanMode::default(),
            ranges: Vec::new(),
            frequencies: Vec::new(),
            lockouts: Vec::new(),
            priority: Vec::new(),
            dwell_ms: default_dwell_ms(),
            resume_ms: default_resume_ms(),
            hold_ms: default_hold_ms(),
            measure_bw_hz: None,
            hardware_sweep: default_hardware_sweep(),
            margin_db: default_margin_db(),
        }
    }

    #[must_use]
    pub fn problem(&self) -> Option<&'static str> {
        if !self.margin_db.is_finite() {
            return Some("Margin must be a number");
        }
        if self
            .measure_bw_hz
            .is_some_and(|hz| !hz.is_finite() || hz <= 0.0)
        {
            return Some("Measured bandwidth must be positive");
        }
        for list in [&self.frequencies, &self.lockouts, &self.priority] {
            if list.len() > MAX_SCAN_TARGETS {
                return Some("Too many frequencies");
            }
            if list.iter().any(|hz| !hz.is_finite() || *hz <= 0.0) {
                return Some("A frequency must be a positive number of Hz");
            }
        }
        self.range_problem()
    }

    fn range_problem(&self) -> Option<&'static str> {
        let mut steps = self.frequencies.len() + self.priority.len();
        for range in &self.ranges {
            if !range.start_hz.is_finite() || !range.stop_hz.is_finite() {
                return Some("A range needs two finite bounds");
            }
            if !range.step_hz.is_finite() || range.step_hz <= 0.0 {
                return Some("A range step must be positive");
            }
            if range.stop_hz < range.start_hz {
                return Some("A range ends before it starts");
            }
            let count = (range.stop_hz - range.start_hz) / range.step_hz;
            if !count.is_finite() || count >= MAX_SCAN_TARGETS as f64 {
                return Some("Too many targets; widen the step or narrow the range");
            }
            steps += count as usize + 1;
            if steps > MAX_SCAN_TARGETS {
                return Some("Too many targets; widen the step or narrow the range");
            }
        }
        None
    }
}

impl Default for ScanSettings {
    fn default() -> Self {
        Self {
            ranges: vec![ScanRange {
                start_hz: 145_600_000.0,
                stop_hz: 145_800_000.0,
                step_hz: 12_500.0,
            }],
            ..Self::for_channel(0)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScanState {
    Scanning,
    Holding,
    Done,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ScannerStatus {
    pub state: ScanState,
    pub settings: ScanSettings,
    pub targets: u32,
    #[serde(default)]
    pub first_hz: f64,
    #[serde(default)]
    pub last_hz: f64,
    pub current_hz: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_snr_db: Option<f32>,
    pub sweeps: u64,
    pub hits: u64,
    /// Whether the sweep in force is the radio's own. A scan that asked for one and did not get
    /// it says so here rather than looking like a firmware sweep that is merely slow.
    #[serde(default)]
    pub hardware_sweep: bool,
    /// Whether every target sits in the radio's window at once, so the scan watches them all on
    /// every frame and moves only the decoder, never the radio.
    #[serde(default)]
    pub live: bool,
    /// The frequencies that went busy most recently, newest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub heard: Vec<HeardSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub const MAX_HEARD: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct HeardSignal {
    pub hz: f64,
    pub snr_db: f32,
    pub at_ms: u64,
}

impl ScannerStatus {
    /// Puts `signal` first, dropping an older entry for the same frequency and the oldest past
    /// [`MAX_HEARD`].
    pub fn note_heard(&mut self, signal: HeardSignal) {
        self.heard.retain(|old| old.hz != signal.hz);
        self.heard.insert(0, signal);
        self.heard.truncate(MAX_HEARD);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScanAction {
    Start,
    Stop,
    /// Leaves the frequency the scan is holding on and locks it out.
    Skip,
    /// Holds `hz`, a frequency inside the scan, until it has been quiet a while.
    Jump {
        hz: f64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(edit: impl FnOnce(&mut ScanSettings)) -> Option<&'static str> {
        let mut settings = ScanSettings::default();
        edit(&mut settings);
        settings.problem()
    }

    #[test]
    fn a_fresh_scan_is_savable() {
        assert_eq!(ScanSettings::default().problem(), None);
        assert_eq!(ScanSettings::for_channel(1).problem(), None);
    }

    #[test]
    fn heard_keeps_the_newest_once_and_drops_the_oldest() {
        let mut status = ScannerStatus {
            state: ScanState::Scanning,
            settings: ScanSettings::default(),
            targets: 0,
            first_hz: 0.0,
            last_hz: 0.0,
            current_hz: 0.0,
            current_snr_db: None,
            sweeps: 0,
            hits: 0,
            hardware_sweep: false,
            live: false,
            heard: Vec::new(),
            error: None,
        };
        let at = |hz: f64, at_ms: u64| HeardSignal {
            hz,
            snr_db: 20.0,
            at_ms,
        };
        for i in 0..=MAX_HEARD as u64 {
            status.note_heard(at(i as f64, i));
        }
        status.note_heard(at(3.0, 99));
        let order: Vec<f64> = status.heard.iter().map(|s| s.hz).collect();
        assert_eq!(order, vec![3.0, 8.0, 7.0, 6.0, 5.0, 4.0, 2.0, 1.0]);
        assert_eq!(status.heard[0].at_ms, 99);
    }

    #[test]
    fn a_zero_step_is_refused_by_name() {
        let problem = with(|s| s.ranges[0].step_hz = 0.0).expect("a zero step");
        assert!(problem.contains("step"), "{problem}");
    }

    #[test]
    fn a_range_wider_than_the_target_cap_is_refused() {
        let problem = with(|s| {
            s.ranges[0].start_hz = 0.0;
            s.ranges[0].stop_hz = 1e9;
            s.ranges[0].step_hz = 1.0;
        })
        .expect("too many targets");
        assert!(problem.contains("targets"), "{problem}");
    }

    #[test]
    fn a_backwards_range_and_an_unusable_frequency_are_refused() {
        assert!(
            with(|s| {
                s.ranges[0].start_hz = 200.0;
                s.ranges[0].stop_hz = 100.0;
            })
            .is_some()
        );
        assert!(with(|s| s.frequencies = vec![-1.0]).is_some());
        assert!(with(|s| s.frequencies = vec![f64::NAN]).is_some());
        assert!(with(|s| s.margin_db = f32::NAN).is_some());
        assert!(with(|s| s.measure_bw_hz = Some(0.0)).is_some());
        assert!(with(|s| s.frequencies = vec![1.0; MAX_SCAN_TARGETS + 1]).is_some());
    }
}
