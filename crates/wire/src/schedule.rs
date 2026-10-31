use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub const MAX_SCHEDULE_WINDOWS: usize = 16;
pub const MINUTES_PER_DAY: u16 = 1_440;
pub const MAX_TIME_ZONE_LEN: usize = 64;

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl Weekday {
    pub const ALL: [Self; 7] = [
        Self::Monday,
        Self::Tuesday,
        Self::Wednesday,
        Self::Thursday,
        Self::Friday,
        Self::Saturday,
        Self::Sunday,
    ];
}

/// On from `start_min` to `stop_min`, in minutes past midnight. A stop before the start runs past
/// midnight, and a stop equal to the start keeps it on for a whole day. Days name the day a window
/// opens; none means every day.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ScheduleWindow {
    pub start_min: u16,
    pub stop_min: u16,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub days: Vec<Weekday>,
}

impl ScheduleWindow {
    #[must_use]
    pub fn valid(&self) -> bool {
        self.start_min < MINUTES_PER_DAY
            && self.stop_min < MINUTES_PER_DAY
            && self.days.len() <= Weekday::ALL.len()
            && self
                .days
                .iter()
                .enumerate()
                .all(|(index, day)| !self.days[..index].contains(day))
    }

    #[must_use]
    pub fn opens_on(&self, day: Weekday) -> bool {
        self.days.is_empty() || self.days.contains(&day)
    }

    #[must_use]
    pub const fn length_min(&self) -> u16 {
        if self.stop_min > self.start_min {
            self.stop_min - self.start_min
        } else {
            MINUTES_PER_DAY - self.start_min + self.stop_min
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ScheduleNode {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub windows: Vec<ScheduleWindow>,
    /// An IANA zone such as `Europe/Berlin`. None keeps the server's own clock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,
}

impl ScheduleNode {
    #[must_use]
    pub fn valid(&self) -> bool {
        self.windows.len() <= MAX_SCHEDULE_WINDOWS
            && self.windows.iter().all(ScheduleWindow::valid)
            && self
                .zone
                .as_ref()
                .is_none_or(|zone| !zone.trim().is_empty() && zone.len() <= MAX_TIME_ZONE_LEN)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ScheduleStatus {
    pub node: String,
    pub on: bool,
    /// When it next turns on or off, in Unix seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_change: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub driving: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(start_min: u16, stop_min: u16) -> ScheduleWindow {
        ScheduleWindow {
            start_min,
            stop_min,
            days: Vec::new(),
        }
    }

    #[test]
    fn a_window_past_midnight_and_a_whole_day_have_their_length() {
        assert_eq!(window(60, 120).length_min(), 60);
        assert_eq!(window(1_380, 60).length_min(), 120);
        assert_eq!(window(300, 300).length_min(), MINUTES_PER_DAY);
    }

    #[test]
    fn a_window_refuses_minutes_past_the_day_and_doubled_days() {
        assert!(!window(MINUTES_PER_DAY, 0).valid());
        let doubled = ScheduleWindow {
            days: vec![Weekday::Monday, Weekday::Monday],
            ..window(0, 60)
        };
        assert!(!doubled.valid());
        assert!(
            !ScheduleNode {
                zone: Some(" ".to_owned()),
                ..ScheduleNode::default()
            }
            .valid()
        );
    }
}
