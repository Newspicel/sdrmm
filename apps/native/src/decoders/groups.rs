use std::{cmp::Ordering, sync::Arc};

use sdrmm_wire::rest::{DecoderLogGroup, DecoderLogQuery, LogGroupKey};

use crate::decoded::time_ms;

use super::log::{LogRow, query_string, stored_row};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogView {
    List,
    Frequency,
    Station,
}

pub const VIEW_CHOICES: [(LogView, &str); 3] = [
    (LogView::List, "List"),
    (LogView::Frequency, "Frequency"),
    (LogView::Station, "Station"),
];

impl LogView {
    #[must_use]
    pub const fn of(group: Option<LogGroupKey>) -> Self {
        match group {
            None => Self::List,
            Some(LogGroupKey::Frequency) => Self::Frequency,
            Some(LogGroupKey::Station) => Self::Station,
        }
    }

    #[must_use]
    pub const fn group(self) -> Option<LogGroupKey> {
        match self {
            Self::List => None,
            Self::Frequency => Some(LogGroupKey::Frequency),
            Self::Station => Some(LogGroupKey::Station),
        }
    }
}

#[must_use]
pub const fn group_name(by: LogGroupKey) -> &'static str {
    match by {
        LogGroupKey::Frequency => "frequency",
        LogGroupKey::Station => "station",
    }
}

#[must_use]
pub fn groups_path(by: LogGroupKey, query: &DecoderLogQuery) -> String {
    format!(
        "/api/decoderlog/groups/{}?{}",
        group_name(by),
        query_string(query)
    )
}

#[derive(Clone, Debug, PartialEq)]
pub struct GroupRow {
    pub key: String,
    pub label: String,
    pub kind: String,
    pub count: u64,
    pub airtime_ms: Option<u64>,
    pub first_at: String,
    pub last_at: String,
    pub latest: LogRow,
}

#[must_use]
pub fn format_mhz(hz: f64) -> String {
    format!("{:.4} MHz", hz / 1e6)
}

fn group_key(latest: &LogRow, by: LogGroupKey) -> String {
    match by {
        LogGroupKey::Frequency => format!("frequency:{}", (latest.freq_hz / 100.0).round()),
        LogGroupKey::Station => format!(
            "station:{}:{}",
            latest.kind,
            latest.station.as_deref().unwrap_or_default()
        ),
    }
}

#[must_use]
pub fn group_rows(groups: &[DecoderLogGroup], by: LogGroupKey) -> Vec<GroupRow> {
    groups
        .iter()
        .map(|group| {
            let latest = stored_row(Arc::new(group.latest.clone()));
            GroupRow {
                key: group_key(&latest, by),
                label: match by {
                    LogGroupKey::Frequency => format_mhz(latest.freq_hz),
                    LogGroupKey::Station => {
                        latest.station.clone().unwrap_or_else(|| "-".to_owned())
                    }
                },
                kind: latest.kind.clone(),
                count: group.count,
                airtime_ms: group.airtime_ms,
                first_at: group.first_at.clone(),
                last_at: group.last_at.clone(),
                latest,
            }
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupSort {
    Key,
    Count,
    Last,
}

fn recent(a: &GroupRow, b: &GroupRow) -> Ordering {
    let at = |row: &GroupRow| time_ms(&row.last_at).unwrap_or(0);
    at(b).cmp(&at(a))
}

#[must_use]
pub fn sort_groups(rows: &[GroupRow], sort: GroupSort, by: LogGroupKey) -> Vec<GroupRow> {
    let mut sorted = rows.to_vec();
    sorted.sort_by(|a, b| {
        let order = match sort {
            GroupSort::Key if by == LogGroupKey::Frequency => {
                a.latest.freq_hz.total_cmp(&b.latest.freq_hz)
            }
            GroupSort::Key => a.label.to_lowercase().cmp(&b.label.to_lowercase()),
            GroupSort::Count => b.count.cmp(&a.count),
            GroupSort::Last => Ordering::Equal,
        };
        order.then_with(|| recent(a, b))
    });
    sorted
}

#[must_use]
pub fn has_airtime(rows: &[GroupRow]) -> bool {
    rows.iter().any(|row| row.airtime_ms.is_some())
}

#[must_use]
pub fn format_duration(seconds: f64) -> String {
    let tenths = (seconds * 10.0).round() / 10.0;
    if tenths < 60.0 {
        return format!("{tenths:.1} s");
    }
    let whole = tenths.round() as u64;
    let (h, m, s) = (whole / 3600, (whole % 3600) / 60, whole % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[must_use]
pub fn format_airtime(ms: Option<u64>) -> String {
    ms.map_or_else(|| "-".to_owned(), |ms| format_duration(ms as f64 / 1000.0))
}

#[cfg(test)]
mod tests;
