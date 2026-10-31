use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use jiff::{
    Span, Timestamp,
    civil::{self, Date},
    tz::TimeZone,
};
use sdrmm_wire::{
    ControlAck, ControlCommand, NodeBody, ScheduleNode, ScheduleStatus, ScheduleWindow,
    ServerEvent, Weekday,
};

use crate::AppState;

const TICK: Duration = Duration::from_secs(1);
const LOOK_BACK_DAYS: i64 = 1;
const LOOK_AHEAD_DAYS: i64 = 8;
const UNKNOWN_ZONE: &str = "Unknown time zone";

#[derive(Default)]
pub(crate) struct ScheduleHub {
    statuses: Mutex<HashMap<String, ScheduleStatus>>,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl ScheduleHub {
    pub(crate) fn snapshot(&self) -> Vec<ServerEvent> {
        locked(&self.statuses)
            .values()
            .map(|status| ServerEvent::ScheduleUpdate {
                status: Box::new(status.clone()),
            })
            .collect()
    }
}

/// Says on or off to every wired radio and recorder once a second.
pub(crate) fn follow(state: &AppState) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        tracing::warn!("no runtime in context: schedules will not run");
        return;
    };
    let state = state.clone();
    handle.spawn(async move {
        let mut ticks = tokio::time::interval(TICK);
        ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticks.tick().await;
            let app = state.clone();
            if let Err(error) = tokio::task::spawn_blocking(move || tick(&app)).await {
                tracing::error!(%error, "schedules stopped");
                return;
            }
        }
    });
}

fn tick(state: &AppState) {
    let resolved = state.resolved.current();
    let now = Timestamp::now();
    let mut statuses = HashMap::new();
    for node in &resolved.graph.nodes {
        let NodeBody::Schedule(schedule) = &node.body else {
            continue;
        };
        let mut status = status_at(&node.id, schedule, now);
        let command = match status.error {
            Some(_) => ControlCommand::Release,
            None => ControlCommand::Run { on: status.on },
        };
        let acks = state
            .control
            .deliver(&state.engine, &resolved.patch, &node.id, &command);
        take_acks(&mut status, acks);
        statuses.insert(node.id.clone(), status);
    }
    let changed: Vec<ScheduleStatus> = {
        let mut held = locked(&state.schedules.statuses);
        let changed = statuses
            .values()
            .filter(|status| held.get(&status.node) != Some(status))
            .cloned()
            .collect();
        *held = statuses;
        changed
    };
    for status in changed {
        state.engine.emit_event(ServerEvent::ScheduleUpdate {
            status: Box::new(status),
        });
    }
}

fn take_acks(status: &mut ScheduleStatus, acks: Vec<ControlAck>) {
    for ack in acks {
        match ack.error {
            Some(error) => status.error = Some(format!("{}: {error}", ack.target)),
            None if !status.driving.contains(&ack.target) => status.driving.push(ack.target),
            None => {}
        }
    }
    status.driving.sort();
}

fn status_at(node: &str, schedule: &ScheduleNode, now: Timestamp) -> ScheduleStatus {
    let mut status = ScheduleStatus {
        node: node.to_owned(),
        ..ScheduleStatus::default()
    };
    let zone = match &schedule.zone {
        Some(name) => TimeZone::get(name),
        None => Ok(TimeZone::system()),
    };
    let Ok(zone) = zone else {
        status.error = Some(UNKNOWN_ZONE.to_owned());
        return status;
    };
    status.zone = zone.iana_name().map(str::to_owned);
    let (on, next) = state_at(&schedule.windows, &zone, now);
    status.on = on;
    status.next_change = next.map(Timestamp::as_second);
    status
}

/// Whether a window holds `now`, and when that next flips. Overlapping and back to back windows
/// count as one stretch.
fn state_at(
    windows: &[ScheduleWindow],
    zone: &TimeZone,
    now: Timestamp,
) -> (bool, Option<Timestamp>) {
    let mut stretches = stretches(windows, zone, now.to_zoned(zone.clone()).date());
    stretches.sort_unstable();
    let mut end: Option<Timestamp> = None;
    for &(start, stop) in &stretches {
        match end {
            Some(held) if start <= held => end = Some(held.max(stop)),
            Some(_) => break,
            None if start <= now && now < stop => end = Some(stop),
            None => {}
        }
    }
    match end {
        Some(stop) => (true, Some(stop)),
        None => (
            false,
            stretches
                .iter()
                .map(|&(start, _)| start)
                .find(|&start| start > now),
        ),
    }
}

fn stretches(
    windows: &[ScheduleWindow],
    zone: &TimeZone,
    today: Date,
) -> Vec<(Timestamp, Timestamp)> {
    (-LOOK_BACK_DAYS..=LOOK_AHEAD_DAYS)
        .filter_map(|offset| today.checked_add(Span::new().days(offset)).ok())
        .flat_map(|day| {
            windows
                .iter()
                .filter(move |window| window.opens_on(weekday(day)))
                .filter_map(move |window| stretch(window, zone, day))
        })
        .collect()
}

fn stretch(window: &ScheduleWindow, zone: &TimeZone, day: Date) -> Option<(Timestamp, Timestamp)> {
    let start = at(day, window.start_min, zone)?;
    let days_on = i64::from((window.start_min + window.length_min()) / sdrmm_wire::MINUTES_PER_DAY);
    let stop_day = day.checked_add(Span::new().days(days_on)).ok()?;
    let stop = at(stop_day, window.stop_min, zone)?;
    Some((start, stop))
}

fn at(day: Date, minute: u16, zone: &TimeZone) -> Option<Timestamp> {
    let hour = i8::try_from(minute / 60).ok()?;
    let minute = i8::try_from(minute % 60).ok()?;
    day.at(hour, minute, 0, 0)
        .to_zoned(zone.clone())
        .ok()
        .map(|zoned| zoned.timestamp())
}

fn weekday(day: Date) -> Weekday {
    match day.weekday() {
        civil::Weekday::Monday => Weekday::Monday,
        civil::Weekday::Tuesday => Weekday::Tuesday,
        civil::Weekday::Wednesday => Weekday::Wednesday,
        civil::Weekday::Thursday => Weekday::Thursday,
        civil::Weekday::Friday => Weekday::Friday,
        civil::Weekday::Saturday => Weekday::Saturday,
        civil::Weekday::Sunday => Weekday::Sunday,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(start_min: u16, stop_min: u16, days: &[Weekday]) -> ScheduleWindow {
        ScheduleWindow {
            start_min,
            stop_min,
            days: days.to_vec(),
        }
    }

    fn utc(text: &str) -> Timestamp {
        text.parse().expect("a timestamp")
    }

    #[test]
    fn a_window_holds_from_its_start_to_its_stop() {
        let windows = [window(8 * 60, 9 * 60, &[])];
        assert_eq!(
            state_at(&windows, &TimeZone::UTC, utc("2026-10-28T08:30:00Z")),
            (true, Some(utc("2026-10-28T09:00:00Z")))
        );
        assert_eq!(
            state_at(&windows, &TimeZone::UTC, utc("2026-10-28T09:00:00Z")),
            (false, Some(utc("2026-10-29T08:00:00Z")))
        );
    }

    #[test]
    fn a_window_past_midnight_holds_into_the_next_day() {
        let windows = [window(22 * 60, 2 * 60, &[Weekday::Friday])];
        assert_eq!(
            state_at(&windows, &TimeZone::UTC, utc("2026-10-31T01:00:00Z")),
            (true, Some(utc("2026-10-31T02:00:00Z"))),
            "Friday's window still holds early on Saturday"
        );
        assert_eq!(
            state_at(&windows, &TimeZone::UTC, utc("2026-10-31T23:00:00Z")),
            (false, Some(utc("2026-11-06T22:00:00Z")))
        );
    }

    #[test]
    fn touching_windows_are_one_stretch() {
        let windows = [window(60, 120, &[]), window(120, 180, &[])];
        assert_eq!(
            state_at(&windows, &TimeZone::UTC, utc("2026-10-28T01:30:00Z")),
            (true, Some(utc("2026-10-28T03:00:00Z")))
        );
    }

    #[test]
    fn a_window_is_read_on_the_schedule_s_own_clock() {
        let berlin = TimeZone::get("Europe/Berlin").expect("Berlin");
        let windows = [window(8 * 60, 9 * 60, &[])];
        assert_eq!(
            state_at(&windows, &berlin, utc("2026-07-01T06:30:00Z")),
            (true, Some(utc("2026-07-01T07:00:00Z")))
        );
    }

    #[test]
    fn no_window_never_turns_on_and_a_bad_zone_says_so() {
        assert_eq!(
            state_at(&[], &TimeZone::UTC, utc("2026-10-28T01:30:00Z")),
            (false, None)
        );
        let lost = ScheduleNode {
            windows: Vec::new(),
            zone: Some("Mars/Olympus".to_owned()),
        };
        assert_eq!(
            status_at("s", &lost, Timestamp::now()).error.as_deref(),
            Some(UNKNOWN_ZONE)
        );
    }
}
