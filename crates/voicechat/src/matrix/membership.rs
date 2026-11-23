//! Who is in a call and where to reach their media.

use std::collections::HashSet;

use base64::Engine;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub(crate) const LEGACY_TYPE: &str = "org.matrix.msc3401.call.member";
pub(crate) const RTC_TYPE: &str = "m.rtc.member";
const APPLICATION: &str = "m.call";
const EXPIRES_MS: u64 = 14_400_000;
const LIVEKIT: &str = "livekit";
const LIVEKIT_STABLE: &str = "m.livekit";
const WELL_KNOWN_FOCI: &str = "org.matrix.msc4143.rtc_foci";

/// The `LiveKit` JWT service that hands out tokens for a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Focus {
    pub service_url: String,
}

/// A device that has joined the call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Member {
    pub user: String,
    pub device: String,
    pub membership_id: String,
    pub focus: Option<Focus>,
}

/// A room state event as `(type, state key, content)`.
pub(crate) type StateEntry = (String, String, Value);

/// A call member state event and the `origin_server_ts` the server gave it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CallState {
    pub entry: StateEntry,
    pub sent_ms: u64,
}

fn focus_of(transport: &Value) -> Option<Focus> {
    let kind = transport.get("type")?.as_str()?;
    if kind != LIVEKIT && kind != LIVEKIT_STABLE {
        return None;
    }
    let service_url = transport.get("livekit_service_url")?.as_str()?;
    Some(Focus {
        service_url: service_url.to_owned(),
    })
}

fn foci_in(list: Option<&Value>) -> Vec<Focus> {
    let items = list
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    items.iter().filter_map(focus_of).collect()
}

/// Reads `GET /_matrix/client/v1/rtc/transports` (`transports`) and its
/// unstable form (`rtc_transports`).
pub(crate) fn foci_from_transports(json: &Value) -> Vec<Focus> {
    foci_in(
        json.get("transports")
            .or_else(|| json.get("rtc_transports")),
    )
}

pub(crate) fn foci_from_well_known(json: &Value) -> Vec<Focus> {
    foci_in(json.get(WELL_KNOWN_FOCI))
}

fn published_focus(transports: Option<&Value>) -> Option<Focus> {
    let list = transports?.as_array()?;
    let stable = list
        .iter()
        .find(|t| t.get("type").and_then(Value::as_str) == Some(LIVEKIT_STABLE))
        .and_then(focus_of);
    stable.or_else(|| list.iter().find_map(focus_of))
}

struct Parsed {
    member: Member,
    created_ts: u64,
}

fn str_field<'a>(content: &'a Value, key: &str) -> Option<&'a str> {
    content.get(key)?.as_str()
}

fn legacy_user(state_key: &str, device: &str, call_id: &str) -> Option<String> {
    let suffix = format!("_{device}_{APPLICATION}{call_id}");
    let user = state_key.trim_start_matches('_').strip_suffix(&suffix)?;
    user.starts_with('@').then(|| user.to_owned())
}

/// An entry without `created_ts` dates from `sent_ms`, as js-sdk `CallMembership` reads it.
fn parse_legacy(state_key: &str, content: &Value, sent_ms: u64, now_ms: u64) -> Option<Parsed> {
    if str_field(content, "application")? != APPLICATION {
        return None;
    }
    let call_id = str_field(content, "call_id")?;
    if !matches!(call_id, "" | "ROOM") {
        return None;
    }
    let device = str_field(content, "device_id")?;
    let created_ts = content
        .get("created_ts")
        .and_then(Value::as_u64)
        .unwrap_or(sent_ms);
    let expires = content
        .get("expires")
        .and_then(Value::as_u64)
        .unwrap_or(EXPIRES_MS);
    if created_ts.saturating_add(expires) <= now_ms {
        return None;
    }
    let user = legacy_user(state_key, device, call_id)?;
    let membership_id = str_field(content, "membershipID")
        .map_or_else(|| format!("{user}:{device}"), str::to_owned);
    Some(Parsed {
        member: Member {
            user,
            device: device.to_owned(),
            membership_id,
            focus: published_focus(content.get("foci_preferred")),
        },
        created_ts,
    })
}

fn is_left(content: &Value) -> bool {
    let membership = content
        .pointer("/member/membership")
        .and_then(Value::as_str);
    let only_slot = content.as_object().is_some_and(|fields| {
        fields
            .keys()
            .all(|key| key == "slot_id" || key == "msc4354_sticky_key")
    });
    membership == Some("leave") || only_slot
}

fn parse_rtc(content: &Value, sent_ms: u64) -> Option<Parsed> {
    if is_left(content) {
        return None;
    }
    if !matches!(
        str_field(content, "slot_id")?,
        "m.call#ROOM" | "m.call#room"
    ) {
        return None;
    }
    let member = content.get("member")?;
    Some(Parsed {
        member: Member {
            user: str_field(member, "user_id")?.to_owned(),
            device: str_field(member, "device_id")?.to_owned(),
            membership_id: str_field(member, "id")?.to_owned(),
            focus: published_focus(content.pointer("/transports/published")),
        },
        created_ts: sent_ms,
    })
}

fn parse_entry(state: &CallState, now_ms: u64) -> Option<Parsed> {
    let (kind, state_key, content) = &state.entry;
    match kind.as_str() {
        LEGACY_TYPE => parse_legacy(state_key, content, state.sent_ms, now_ms),
        RTC_TYPE => parse_rtc(content, state.sent_ms),
        _ => None,
    }
}

/// Live members of the call, oldest first, without `skip` (our own user and device).
/// A device with both a legacy and a new membership counts once, at its newest.
pub(crate) fn members(states: &[CallState], now_ms: u64, skip: (&str, &str)) -> Vec<Member> {
    let mut parsed: Vec<Parsed> = states
        .iter()
        .filter_map(|state| parse_entry(state, now_ms))
        .filter(|p| (p.member.user.as_str(), p.member.device.as_str()) != skip)
        .collect();
    parsed.sort_by_key(|p| std::cmp::Reverse(p.created_ts));
    let mut seen = HashSet::new();
    parsed.retain(|p| seen.insert((p.member.user.clone(), p.member.device.clone())));
    parsed.reverse();
    parsed.into_iter().map(|p| p.member).collect()
}

fn state_key(user: &str, device: &str, owned_keys: bool) -> String {
    let key = format!("{user}_{device}_{APPLICATION}");
    if owned_keys { key } else { format!("_{key}") }
}

/// The join event in the form matrix-js-sdk sends: a legacy session membership with
/// `multi_sfu`, so every member publishes through the service it names.
/// `owned_keys` is true when the room version has MSC3757 owned state keys.
pub(crate) fn our_membership(
    user: &str,
    device: &str,
    focus: &Focus,
    now_ms: u64,
    owned_keys: bool,
) -> StateEntry {
    let content = json!({
        "application": APPLICATION,
        "call_id": "",
        "scope": "m.room",
        "device_id": device,
        "membershipID": format!("{user}:{device}"),
        "expires": EXPIRES_MS,
        "created_ts": now_ms,
        "focus_active": {"type": LIVEKIT, "focus_selection": "multi_sfu"},
        "foci_preferred": [{"type": LIVEKIT, "livekit_service_url": focus.service_url}],
    });
    (
        LEGACY_TYPE.to_owned(),
        state_key(user, device, owned_keys),
        content,
    )
}

/// When our membership lapses: `created_ts` plus `expires`.
pub(crate) fn expires_at((_, _, content): &StateEntry) -> Option<u64> {
    let created = content.get("created_ts")?.as_u64()?;
    let expires = content.get("expires")?.as_u64()?;
    Some(created.saturating_add(expires))
}

/// Keeps our membership one more expiry period, as js-sdk `MembershipManager` does: same
/// `created_ts`, larger `expires`.
pub(crate) fn extend((_, _, content): &mut StateEntry) {
    let expires = content
        .get("expires")
        .and_then(Value::as_u64)
        .unwrap_or(EXPIRES_MS);
    content["expires"] = json!(expires.saturating_add(EXPIRES_MS));
}

pub(crate) fn our_leave(user: &str, device: &str, owned_keys: bool) -> StateEntry {
    (
        LEGACY_TYPE.to_owned(),
        state_key(user, device, owned_keys),
        json!({}),
    )
}

fn hashed(parts: &[&str]) -> String {
    let encoded = serde_json::to_vec(parts).unwrap_or_default();
    STANDARD_NO_PAD.encode(Sha256::digest(encoded))
}

/// Every `LiveKit` identity `lk-jwt-service` may give this member: the plain
/// `user:device` of older services, then the hashed forms of the current one
/// (`[user, member id]` and the legacy `[user, device, member id]`).
pub(crate) fn identities(member: &Member) -> Vec<String> {
    let Member {
        user,
        device,
        membership_id,
        ..
    } = member;
    let mut all = vec![
        format!("{user}:{device}"),
        hashed(&[user, membership_id]),
        hashed(&[user, device, membership_id]),
    ];
    all.dedup();
    all
}

/// The focus of the oldest member that names one, else ours.
pub(crate) fn active_focus(members: &[Member], ours: &Focus) -> Focus {
    members
        .iter()
        .find_map(|m| m.focus.clone())
        .unwrap_or_else(|| ours.clone())
}

#[cfg(test)]
mod tests;
