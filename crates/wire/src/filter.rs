use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    decode::{Capability, DecoderEvent, EventKind, KindSet},
    present::{Presentation, VoiceFacts},
};

pub const MAX_FILTER_KINDS: usize = 64;
pub const MAX_FILTER_IDS: usize = 256;
pub const MAX_FILTER_DURATION_MS: u32 = 600_000;
pub const MAX_FILTER_TEXT_LEN: usize = 128;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FilterMode {
    #[default]
    Keep,
    Drop,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct EventFilterNode {
    #[serde(default)]
    pub mode: FilterMode,
    #[serde(default)]
    pub kinds: Vec<EventKind>,
    #[serde(default)]
    pub stations: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contains: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_position: Option<bool>,
    #[serde(default)]
    pub talkgroups: Vec<u32>,
    #[serde(default)]
    pub radios: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encrypted: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emergency: Option<bool>,
    #[serde(default)]
    pub min_duration_ms: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Matched,
    Missed,
    Unjudged,
}

impl Verdict {
    fn of(matched: bool) -> Self {
        if matched { Self::Matched } else { Self::Missed }
    }
}

impl EventFilterNode {
    #[must_use]
    pub fn valid(&self) -> bool {
        self.kinds.len() <= MAX_FILTER_KINDS
            && self.stations.len() <= MAX_FILTER_IDS
            && self.talkgroups.len() <= MAX_FILTER_IDS
            && self.radios.len() <= MAX_FILTER_IDS
            && self.min_duration_ms <= MAX_FILTER_DURATION_MS
            && self
                .contains
                .as_ref()
                .is_none_or(|text| text.len() <= MAX_FILTER_TEXT_LEN)
            && self
                .stations
                .iter()
                .all(|station| !station.is_empty() && station.len() <= MAX_FILTER_TEXT_LEN)
    }

    #[must_use]
    pub fn narrow(&self, offered: KindSet) -> KindSet {
        let listed: KindSet = self.kinds.iter().copied().collect();
        match self.mode {
            FilterMode::Keep if listed.is_empty() => offered,
            FilterMode::Keep => offered.intersection(listed),
            FilterMode::Drop if self.judges_only_kinds() => offered.without(listed),
            FilterMode::Drop => offered,
        }
    }

    fn judges_only_kinds(&self) -> bool {
        self.stations.is_empty()
            && self.contains.as_deref().is_none_or(str::is_empty)
            && self.has_position.is_none()
            && self.talkgroups.is_empty()
            && self.radios.is_empty()
            && self.encrypted.is_none()
            && self.emergency.is_none()
            && self.min_duration_ms == 0
    }

    #[must_use]
    pub fn passes(&self, event: &DecoderEvent) -> bool {
        self.admits(event.kind(), &event.present())
    }

    #[must_use]
    pub fn admits(&self, kind: EventKind, view: &Presentation) -> bool {
        let mut judged = false;
        for verdict in self.verdicts(kind, view) {
            match verdict {
                Verdict::Missed => return self.mode == FilterMode::Drop,
                Verdict::Matched => judged = true,
                Verdict::Unjudged => {}
            }
        }
        match self.mode {
            FilterMode::Keep => true,
            FilterMode::Drop => !judged,
        }
    }

    fn verdicts(&self, kind: EventKind, view: &Presentation) -> [Verdict; 9] {
        let voice = view.voice.as_ref();
        [
            self.kind_verdict(kind),
            self.station_verdict(view),
            self.text_verdict(view),
            self.position_verdict(kind, view),
            list_verdict(&self.talkgroups, voice, |v| v.destination),
            list_verdict(&self.radios, voice, |v| v.source),
            flag_verdict(self.encrypted, voice, |v| v.encrypted),
            flag_verdict(self.emergency, voice, |v| v.emergency),
            self.duration_verdict(view),
        ]
    }

    fn kind_verdict(&self, kind: EventKind) -> Verdict {
        if self.kinds.is_empty() {
            return Verdict::Unjudged;
        }
        Verdict::of(self.kinds.contains(&kind))
    }

    fn station_verdict(&self, view: &Presentation) -> Verdict {
        if self.stations.is_empty() {
            return Verdict::Unjudged;
        }
        let Some(station) = view.entity.as_deref() else {
            return Verdict::Missed;
        };
        Verdict::of(
            self.stations
                .iter()
                .any(|wanted| wanted.eq_ignore_ascii_case(station)),
        )
    }

    fn text_verdict(&self, view: &Presentation) -> Verdict {
        match self.contains.as_deref() {
            None | Some("") => Verdict::Unjudged,
            Some(text) => Verdict::of(view.summary.to_lowercase().contains(&text.to_lowercase())),
        }
    }

    fn position_verdict(&self, kind: EventKind, view: &Presentation) -> Verdict {
        let Some(want) = self.has_position else {
            return Verdict::Unjudged;
        };
        if !kind.has(Capability::Geo) {
            return Verdict::Unjudged;
        }
        Verdict::of(view.place.is_some() == want)
    }

    fn duration_verdict(&self, view: &Presentation) -> Verdict {
        if self.min_duration_ms == 0 {
            return Verdict::Unjudged;
        }
        match view.duration_ms {
            None => Verdict::Unjudged,
            Some(held) => Verdict::of(held >= u64::from(self.min_duration_ms)),
        }
    }
}

fn list_verdict(
    wanted: &[u32],
    voice: Option<&VoiceFacts>,
    pick: fn(&VoiceFacts) -> Option<u32>,
) -> Verdict {
    if wanted.is_empty() {
        return Verdict::Unjudged;
    }
    let Some(voice) = voice else {
        return Verdict::Unjudged;
    };
    Verdict::of(pick(voice).is_some_and(|id| wanted.contains(&id)))
}

fn flag_verdict(
    want: Option<bool>,
    voice: Option<&VoiceFacts>,
    pick: fn(&VoiceFacts) -> Option<bool>,
) -> Verdict {
    let Some(want) = want else {
        return Verdict::Unjudged;
    };
    let Some(voice) = voice else {
        return Verdict::Unjudged;
    };
    Verdict::of(pick(voice) == Some(want))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AdsbMessage, DvFrame, DvFrameKind, DvMode, RttyText, rest::VoiceCall};

    fn call() -> VoiceCall {
        VoiceCall {
            id: 1,
            node: "dmr".to_owned(),
            started_at: "2026-08-16T10:00:00Z".to_owned(),
            ended_at: "2026-08-16T10:00:02Z".to_owned(),
            duration_ms: 2_000,
            device_set: 1,
            channel: 2,
            freq_hz: 451_125_000.0,
            mode: "dmr".to_owned(),
            slot: Some(2),
            color_code: Some(3),
            source: Some(2_621_001),
            destination: Some(505),
            group_call: Some(true),
            encrypted: false,
            emergency: false,
            audio: None,
            audio_error: None,
        }
    }

    fn adsb(icao: &str, callsign: Option<&str>, position: bool) -> DecoderEvent {
        DecoderEvent::Adsb(AdsbMessage {
            icao: icao.to_owned(),
            df: 17,
            callsign: callsign.map(str::to_owned),
            lat: position.then_some(50.4),
            lon: position.then_some(6.6),
            ..AdsbMessage::default()
        })
    }

    fn rtty() -> DecoderEvent {
        DecoderEvent::Rtty(RttyText {
            text: "CQ TEST".to_owned(),
        })
    }

    fn voice_frame() -> DecoderEvent {
        DecoderEvent::Dv(DvFrame {
            source: Some(2_621_001),
            destination: Some(505),
            encrypted: Some(false),
            ..DvFrame::new(DvMode::Dmr, DvFrameKind::Voice)
        })
    }

    #[test]
    fn an_empty_filter_passes_everything() {
        let filter = EventFilterNode::default();
        assert!(filter.passes(&rtty()));
        assert!(filter.passes(&DecoderEvent::Call(call())));
        assert!(filter.passes(&adsb("3C6444", None, true)));
    }

    #[test]
    fn naming_kinds_admits_only_those_kinds() {
        let filter = EventFilterNode {
            kinds: vec![EventKind::Call],
            ..EventFilterNode::default()
        };
        assert!(filter.passes(&DecoderEvent::Call(call())));
        assert!(!filter.passes(&rtty()));
        assert!(!filter.passes(&voice_frame()));
    }

    #[test]
    fn a_station_list_works_for_any_kind_that_names_one() {
        let filter = EventFilterNode {
            stations: vec!["3C6444".to_owned()],
            ..EventFilterNode::default()
        };
        assert!(filter.passes(&adsb("3C6444", None, true)));
        assert!(!filter.passes(&adsb("4CA2D4", None, true)));
        assert!(
            !filter.passes(&rtty()),
            "an event with no station cannot be one of the named ones"
        );
        assert!(
            !filter.passes(&DecoderEvent::Call(call())),
            "a call names its radio, which is not in the list"
        );
    }

    #[test]
    fn a_station_list_ignores_case() {
        let filter = EventFilterNode {
            stations: vec!["3c6444".to_owned()],
            ..EventFilterNode::default()
        };
        assert!(filter.passes(&adsb("3C6444", None, true)));
    }

    #[test]
    fn contains_searches_the_summary_of_any_kind() {
        let filter = EventFilterNode {
            contains: Some("baw".to_owned()),
            ..EventFilterNode::default()
        };
        assert!(filter.passes(&adsb("3C6444", Some("BAW890"), true)));
        assert!(!filter.passes(&adsb("3C6444", Some("RYR9AB"), true)));
    }

    #[test]
    fn a_position_predicate_keeps_only_the_fixes() {
        let without = EventFilterNode {
            has_position: Some(false),
            ..EventFilterNode::default()
        };
        assert!(without.passes(&adsb("3C6444", None, false)));
        assert!(!without.passes(&adsb("3C6444", None, true)));
        assert!(without.passes(&rtty()));
    }

    #[test]
    fn the_voice_predicates_reach_raw_frames_as_well_as_calls() {
        let filter = EventFilterNode {
            talkgroups: vec![505],
            ..EventFilterNode::default()
        };
        assert!(filter.passes(&DecoderEvent::Call(call())));
        assert!(filter.passes(&voice_frame()));
        assert!(!filter.passes(&DecoderEvent::Call(VoiceCall {
            destination: Some(77),
            ..call()
        })));
    }

    #[test]
    fn a_radio_list_admits_only_those_radios() {
        let filter = EventFilterNode {
            radios: vec![2_621_001],
            ..EventFilterNode::default()
        };
        assert!(filter.passes(&DecoderEvent::Call(call())));
        assert!(!filter.passes(&DecoderEvent::Call(VoiceCall {
            source: Some(9),
            ..call()
        })));
    }

    #[test]
    fn the_encryption_and_emergency_flags_are_three_state() {
        let clear = EventFilterNode {
            encrypted: Some(false),
            ..EventFilterNode::default()
        };
        assert!(clear.passes(&DecoderEvent::Call(call())));
        assert!(!clear.passes(&DecoderEvent::Call(VoiceCall {
            encrypted: true,
            ..call()
        })));

        let urgent = EventFilterNode {
            emergency: Some(true),
            ..EventFilterNode::default()
        };
        assert!(!urgent.passes(&DecoderEvent::Call(call())));
        assert!(urgent.passes(&DecoderEvent::Call(VoiceCall {
            emergency: true,
            ..call()
        })));
    }

    #[test]
    fn a_minimum_duration_only_judges_what_has_one() {
        let filter = EventFilterNode {
            min_duration_ms: 1_500,
            ..EventFilterNode::default()
        };
        assert!(filter.passes(&DecoderEvent::Call(call())));
        assert!(!filter.passes(&DecoderEvent::Call(VoiceCall {
            duration_ms: 400,
            ..call()
        })));
        assert!(
            filter.passes(&voice_frame()),
            "a raw frame has no duration to judge"
        );
    }

    #[test]
    fn voice_predicates_leave_other_kinds_alone() {
        let filter = EventFilterNode {
            talkgroups: vec![505],
            radios: vec![1],
            encrypted: Some(true),
            emergency: Some(true),
            min_duration_ms: 60_000,
            ..EventFilterNode::default()
        };
        assert!(
            filter.passes(&adsb("3C6444", None, true)),
            "a voice predicate must not silently drop unrelated kinds"
        );
    }

    #[test]
    fn every_predicate_has_to_agree() {
        let filter = EventFilterNode {
            kinds: vec![EventKind::Call],
            talkgroups: vec![505],
            min_duration_ms: 1_000,
            ..EventFilterNode::default()
        };
        assert!(filter.passes(&DecoderEvent::Call(call())));
        assert!(!filter.passes(&DecoderEvent::Call(VoiceCall {
            duration_ms: 100,
            ..call()
        })));
    }

    #[test]
    fn a_position_rule_leaves_kinds_without_a_position_alone() {
        let fixed = EventFilterNode {
            has_position: Some(true),
            ..EventFilterNode::default()
        };
        assert!(fixed.passes(&adsb("3C6444", None, true)));
        assert!(!fixed.passes(&adsb("3C6444", None, false)));
        assert!(fixed.passes(&rtty()), "a teleprinter has no fix to judge");
    }

    #[test]
    fn drop_mode_removes_what_matches_and_keeps_the_rest() {
        let filter = EventFilterNode {
            mode: FilterMode::Drop,
            contains: Some("test".to_owned()),
            ..EventFilterNode::default()
        };
        assert!(!filter.passes(&rtty()));
        assert!(filter.passes(&adsb("3C6444", Some("DLH123"), true)));
    }

    #[test]
    fn drop_mode_needs_every_rule_to_match_before_it_drops() {
        let filter = EventFilterNode {
            mode: FilterMode::Drop,
            kinds: vec![EventKind::Rtty],
            contains: Some("nothing here".to_owned()),
            ..EventFilterNode::default()
        };
        assert!(
            filter.passes(&rtty()),
            "the text rule missed, so nothing is dropped"
        );
        assert!(filter.passes(&adsb("3C6444", None, true)));
    }

    #[test]
    fn drop_mode_ignores_rules_that_do_not_apply() {
        let filter = EventFilterNode {
            mode: FilterMode::Drop,
            talkgroups: vec![505],
            ..EventFilterNode::default()
        };
        assert!(!filter.passes(&DecoderEvent::Call(call())));
        assert!(filter.passes(&DecoderEvent::Call(VoiceCall {
            destination: Some(77),
            ..call()
        })));
        assert!(
            filter.passes(&adsb("3C6444", None, true)),
            "an aircraft has no talkgroup"
        );
    }

    #[test]
    fn an_empty_drop_filter_drops_nothing() {
        let filter = EventFilterNode {
            mode: FilterMode::Drop,
            ..EventFilterNode::default()
        };
        assert!(filter.passes(&rtty()));
        assert!(filter.passes(&DecoderEvent::Call(call())));
    }

    #[test]
    fn the_mode_defaults_to_keep_when_absent() {
        let filter: EventFilterNode = serde_json::from_str(r#"{"kinds":["call"]}"#).unwrap();
        assert_eq!(filter.mode, FilterMode::Keep);
        let json = serde_json::to_value(&filter).unwrap();
        assert_eq!(json["mode"], "keep");
    }

    #[test]
    fn an_unknown_kind_is_refused_by_name() {
        let error = serde_json::from_str::<EventFilterNode>(r#"{"kinds":["teletext"]}"#)
            .expect_err("unknown kind");
        assert!(error.to_string().contains("teletext"), "{error}");
    }

    #[test]
    fn a_position_rule_judges_radiosondes_too() {
        let fixed = EventFilterNode {
            has_position: Some(true),
            ..EventFilterNode::default()
        };
        let lost: DecoderEvent = serde_json::from_value(serde_json::json!({
            "kind": "radiosonde",
            "data": { "sonde": "rs41", "serial": "S1", "errors_corrected": 0 },
        }))
        .expect("radiosonde");
        assert!(!fixed.passes(&lost));
    }

    #[test]
    fn oversized_lists_are_refused() {
        assert!(EventFilterNode::default().valid());
        assert!(
            !EventFilterNode {
                talkgroups: vec![0; MAX_FILTER_IDS + 1],
                ..EventFilterNode::default()
            }
            .valid()
        );
        assert!(
            !EventFilterNode {
                kinds: vec![EventKind::Call; MAX_FILTER_KINDS + 1],
                ..EventFilterNode::default()
            }
            .valid()
        );
        assert!(
            !EventFilterNode {
                contains: Some("x".repeat(MAX_FILTER_TEXT_LEN + 1)),
                ..EventFilterNode::default()
            }
            .valid()
        );
        assert!(
            !EventFilterNode {
                min_duration_ms: MAX_FILTER_DURATION_MS + 1,
                ..EventFilterNode::default()
            }
            .valid()
        );
    }

    #[test]
    fn a_filter_narrows_the_kinds_it_can_pass() {
        let offered: KindSet = [EventKind::Adsb, EventKind::Rtty, EventKind::Call]
            .into_iter()
            .collect();
        let only = |kinds: &[EventKind]| kinds.iter().copied().collect::<KindSet>();
        assert_eq!(EventFilterNode::default().narrow(offered), offered);
        let keep = EventFilterNode {
            kinds: vec![EventKind::Adsb, EventKind::Ais],
            ..EventFilterNode::default()
        };
        assert_eq!(keep.narrow(offered), only(&[EventKind::Adsb]));
        let drop = EventFilterNode {
            mode: FilterMode::Drop,
            kinds: vec![EventKind::Rtty],
            ..EventFilterNode::default()
        };
        assert_eq!(
            drop.narrow(offered),
            only(&[EventKind::Adsb, EventKind::Call])
        );
        let judged = EventFilterNode {
            stations: vec!["DL1ABC".to_owned()],
            ..drop
        };
        assert_eq!(judged.narrow(offered), offered);
    }
}
