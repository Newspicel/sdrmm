use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use sdrmm_wire::{DvFrame, HeardTalkgroup, MAX_TRUNK_TALKGROUPS};

/// How long a talkgroup may go unheard before it counts as calling again. A site repeats a
/// grant, and a call repeats its talkgroup, for as long as the call runs.
const CALL_GAP: Duration = Duration::from_secs(3);

struct Calls {
    count: u32,
    last: Instant,
    at: jiff::Timestamp,
}

/// The group talkgroups one system called, the least recently heard dropping out past
/// [`MAX_TRUNK_TALKGROUPS`].
#[derive(Default)]
pub(super) struct HeardTalkgroups {
    heard: HashMap<u32, Calls>,
}

impl HeardTalkgroups {
    pub(super) fn hear(&mut self, talkgroup: u32, now: Instant) {
        if let Some(calls) = self.heard.get_mut(&talkgroup) {
            if now.saturating_duration_since(calls.last) >= CALL_GAP {
                calls.count = calls.count.saturating_add(1);
            }
            calls.last = now;
            calls.at = jiff::Timestamp::now();
            return;
        }
        if self.heard.len() >= MAX_TRUNK_TALKGROUPS {
            self.forget_oldest();
        }
        self.heard.insert(
            talkgroup,
            Calls {
                count: 1,
                last: now,
                at: jiff::Timestamp::now(),
            },
        );
    }

    fn forget_oldest(&mut self) {
        let oldest = self
            .heard
            .iter()
            .min_by_key(|(_, calls)| calls.last)
            .map(|(talkgroup, _)| *talkgroup);
        if let Some(talkgroup) = oldest {
            self.heard.remove(&talkgroup);
        }
    }

    pub(super) fn listed(&self) -> Vec<HeardTalkgroup> {
        let mut listed: Vec<HeardTalkgroup> = self
            .heard
            .iter()
            .map(|(talkgroup, calls)| HeardTalkgroup {
                id: *talkgroup,
                calls: calls.count,
                last_heard: format!("{:.0}", calls.at),
            })
            .collect();
        listed.sort_unstable_by_key(|heard| heard.id);
        listed
    }
}

/// Notes `talkgroup` as heard on `system_node`, making its record only the first time.
pub(super) fn hear(
    systems: &mut HashMap<String, HeardTalkgroups>,
    system_node: &str,
    talkgroup: u32,
    now: Instant,
) {
    if let Some(heard) = systems.get_mut(system_node) {
        heard.hear(talkgroup, now);
        return;
    }
    let mut heard = HeardTalkgroups::default();
    heard.hear(talkgroup, now);
    systems.insert(system_node.to_owned(), heard);
}

/// The talkgroup a traffic burst calls, unless it calls a single radio.
pub(super) fn called_group(frame: &DvFrame) -> Option<u32> {
    frame
        .destination
        .filter(|_| frame.group_call != Some(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn later(start: Instant, seconds: u64) -> Instant {
        start + Duration::from_secs(seconds)
    }

    #[test]
    fn a_repeated_grant_counts_as_one_call() {
        let start = Instant::now();
        let mut heard = HeardTalkgroups::default();
        heard.hear(91, start);
        heard.hear(91, later(start, 1));
        heard.hear(91, later(start, 2));
        assert_eq!(heard.listed()[0].calls, 1);

        heard.hear(91, later(start, 10));
        let listed = heard.listed();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, 91);
        assert_eq!(listed[0].calls, 2);
        assert!(!listed[0].last_heard.is_empty());
    }

    #[test]
    fn the_least_recently_heard_talkgroup_makes_room() {
        let start = Instant::now();
        let mut heard = HeardTalkgroups::default();
        for talkgroup in 0..MAX_TRUNK_TALKGROUPS as u32 {
            heard.hear(talkgroup, later(start, u64::from(talkgroup)));
        }
        heard.hear(0, later(start, 1_000));
        heard.hear(9_999, later(start, 1_001));

        let ids: Vec<u32> = heard
            .listed()
            .iter()
            .map(|talkgroup| talkgroup.id)
            .collect();
        assert_eq!(ids.len(), MAX_TRUNK_TALKGROUPS);
        assert!(ids.contains(&0), "a talkgroup heard again was dropped");
        assert!(!ids.contains(&1), "the oldest talkgroup is still kept");
        assert!(ids.contains(&9_999));
    }

    #[test]
    fn a_private_call_names_no_talkgroup() {
        let frame = |group_call| DvFrame {
            destination: Some(4_001),
            group_call,
            ..DvFrame::new("DMR", sdrmm_wire::DvFrameKind::Header)
        };
        assert_eq!(called_group(&frame(Some(false))), None);
        assert_eq!(called_group(&frame(Some(true))), Some(4_001));
        assert_eq!(called_group(&frame(None)), Some(4_001));
    }
}
