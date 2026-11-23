use super::*;
use serde_json::json;

const NOW: u64 = 1_700_000_000_000;
const LEGACY: &str = "org.matrix.msc3401.call.member";
const RTC: &str = "m.rtc.member";

fn focus(url: &str) -> Focus {
    Focus {
        service_url: url.into(),
    }
}

fn legacy(user: &str, device: &str, created: u64, expires: u64, url: &str) -> StateEntry {
    (
        LEGACY.into(),
        format!("_{user}_{device}_m.call"),
        json!({
            "application": "m.call",
            "call_id": "",
            "scope": "m.room",
            "device_id": device,
            "created_ts": created,
            "expires": expires,
            "focus_active": {"type": "livekit", "focus_selection": "multi_sfu"},
            "foci_preferred": [{"type": "livekit", "livekit_service_url": url}],
        }),
    )
}

fn stamped(states: &[StateEntry]) -> Vec<CallState> {
    states
        .iter()
        .map(|entry| CallState {
            entry: entry.clone(),
            sent_ms: NOW,
        })
        .collect()
}

fn rtc(user: &str, device: &str, id: &str, url: &str) -> StateEntry {
    (
        RTC.into(),
        format!("_{user}_{device}_m.call"),
        json!({
            "slot_id": "m.call#ROOM",
            "application": {"type": "m.call"},
            "member": {"user_id": user, "device_id": device, "id": id, "membership": "join"},
            "transports": {
                "published": [{"type": "livekit", "livekit_service_url": url}],
                "can_subscribe": ["livekit"],
            },
            "sticky_key": id,
        }),
    )
}

#[test]
fn transports_list_livekit_foci() {
    let stable = json!({"transports": [
        {"type": "m.livekit", "livekit_service_url": "https://a.example/lk"},
        {"type": "other", "livekit_service_url": "https://x.example"},
        {"type": "livekit", "url": "wss://only-url.example"},
        {"type": "livekit", "livekit_service_url": "https://b.example/lk"},
    ]});
    let unstable = json!({"rtc_transports": [
        {"type": "livekit", "livekit_service_url": "https://c.example"},
    ]});
    assert_eq!(
        foci_from_transports(&stable),
        [focus("https://a.example/lk"), focus("https://b.example/lk")]
    );
    assert_eq!(
        foci_from_transports(&unstable),
        [focus("https://c.example")]
    );
    assert!(foci_from_transports(&json!({})).is_empty());
}

#[test]
fn well_known_foci_parse() {
    let wk = json!({"org.matrix.msc4143.rtc_foci": [
        {"type": "livekit", "livekit_service_url": "https://lk.example"},
        {"type": "jitsi", "url": "https://meet.example"},
        {"type": "livekit"},
    ]});
    assert_eq!(foci_from_well_known(&wk), [focus("https://lk.example")]);
    assert!(foci_from_well_known(&json!({"m.homeserver": {}})).is_empty());
}

#[test]
fn legacy_and_new_members_both_read() {
    let states = [
        legacy(
            "@a:x.org",
            "AAA",
            NOW - 1000,
            14_400_000,
            "https://lk.x.org",
        ),
        rtc("@b:y.org", "BBB", "member-b", "https://lk.y.org"),
    ];
    let got = members(&stamped(&states), NOW, ("", ""));
    assert_eq!(got.len(), 2);
    let a = got.iter().find(|m| m.user == "@a:x.org").unwrap();
    assert_eq!(a.device, "AAA");
    assert_eq!(a.membership_id, "@a:x.org:AAA");
    assert_eq!(a.focus, Some(focus("https://lk.x.org")));
    let b = got.iter().find(|m| m.user == "@b:y.org").unwrap();
    assert_eq!(b.device, "BBB");
    assert_eq!(b.membership_id, "member-b");
    assert_eq!(b.focus, Some(focus("https://lk.y.org")));
}

#[test]
fn expired_and_empty_members_are_gone() {
    let mut left = rtc("@c:y.org", "CCC", "member-c", "https://lk.y.org");
    left.2 = json!({"slot_id": "m.call#ROOM", "msc4354_sticky_key": "member-c"});
    let states = [
        legacy(
            "@a:x.org",
            "AAA",
            NOW - 20_000_000,
            14_400_000,
            "https://lk.x.org",
        ),
        (
            "org.matrix.msc3401.call.member".into(),
            "_@e:x.org_EEE_m.call".into(),
            json!({}),
        ),
        left,
        legacy("@d:x.org", "DDD", NOW - 10, 14_400_000, "https://lk.x.org"),
    ];
    let got = members(&stamped(&states), NOW, ("", ""));
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].user, "@d:x.org");
}

#[test]
fn our_own_membership_is_not_a_peer() {
    let states = [
        legacy("@me:x.org", "PHONE", NOW, 14_400_000, "https://lk.x.org"),
        legacy("@me:x.org", "LAPTOP", NOW, 14_400_000, "https://lk.x.org"),
    ];
    let got = members(&stamped(&states), NOW, ("@me:x.org", "PHONE"));
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].device, "LAPTOP");
}

#[test]
fn identities_cover_both_forms() {
    let member = Member {
        user: "@a:x.org".into(),
        device: "AAA".into(),
        membership_id: "@a:x.org:AAA".into(),
        focus: None,
    };
    let ids = identities(&member);
    assert_eq!(ids[0], "@a:x.org:AAA");
    assert_eq!(ids.len(), 3);
    for hashed in &ids[1..] {
        assert_eq!(hashed.len(), 43);
        assert!(!hashed.contains('='));
    }
    assert_ne!(ids[1], ids[2]);
    assert_eq!(identities(&member), ids);
}

#[test]
fn hashed_identity_matches_the_service_digest() {
    let member = Member {
        user: "@a:x.org".into(),
        device: "AAA".into(),
        membership_id: "m1".into(),
        focus: None,
    };
    let digest = |parts: &[&str]| {
        use base64::Engine;
        use sha2::Digest;
        let json = serde_json::to_vec(parts).unwrap();
        base64::engine::general_purpose::STANDARD_NO_PAD.encode(sha2::Sha256::digest(json))
    };
    let ids = identities(&member);
    assert!(ids.contains(&digest(&["@a:x.org", "m1"])));
    assert!(ids.contains(&digest(&["@a:x.org", "AAA", "m1"])));
}

#[test]
fn oldest_member_picks_the_focus() {
    let states = [
        legacy(
            "@new:x.org",
            "N",
            NOW - 10,
            14_400_000,
            "https://new.example",
        ),
        legacy(
            "@old:x.org",
            "O",
            NOW - 5000,
            14_400_000,
            "https://old.example",
        ),
    ];
    let got = members(&stamped(&states), NOW, ("", ""));
    assert_eq!(got[0].user, "@old:x.org");
    let ours = focus("https://ours.example");
    assert_eq!(active_focus(&got, &ours), focus("https://old.example"));
    assert_eq!(active_focus(&[], &ours), ours);
}

#[test]
fn our_membership_has_the_current_shape() {
    let ours = focus("https://lk.example");
    let (ty, key, content) = our_membership("@me:x.org", "DEV", &ours, NOW, false);
    assert_eq!(ty, LEGACY);
    assert_eq!(key, "_@me:x.org_DEV_m.call");
    assert_eq!(content["application"], "m.call");
    assert_eq!(content["call_id"], "");
    assert_eq!(content["scope"], "m.room");
    assert_eq!(content["device_id"], "DEV");
    assert_eq!(content["membershipID"], "@me:x.org:DEV");
    assert_eq!(content["expires"], 14_400_000);
    assert_eq!(content["created_ts"], NOW);
    assert_eq!(content["focus_active"]["type"], "livekit");
    assert_eq!(
        content["foci_preferred"][0]["livekit_service_url"],
        "https://lk.example"
    );
    let (_, owned, _) = our_membership("@me:x.org", "DEV", &ours, NOW, true);
    assert_eq!(owned, "@me:x.org_DEV_m.call");
}

#[test]
fn our_membership_reads_back_as_a_member() {
    let ours = focus("https://lk.example");
    let entry = our_membership("@me:x.org", "DEV", &ours, NOW, true);
    let got = members(&stamped(&[entry]), NOW, ("", ""));
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].user, "@me:x.org");
    assert_eq!(got[0].focus, Some(ours));
}

#[test]
fn leaving_clears_the_same_key() {
    let ours = focus("https://lk.example");
    let (_, join_key, _) = our_membership("@me:x.org", "DEV", &ours, NOW, false);
    let (ty, key, content) = our_leave("@me:x.org", "DEV", false);
    assert_eq!((ty.as_str(), key), (LEGACY, join_key));
    assert_eq!(content, json!({}));
}

#[test]
fn a_device_counts_once_at_its_newest_membership() {
    let old = legacy(
        "@a:x.org",
        "AAA",
        NOW - 5000,
        14_400_000,
        "https://old.example",
    );
    let new = rtc("@a:x.org", "AAA", "member-a", "https://new.example");
    let states = [
        CallState {
            entry: old,
            sent_ms: NOW - 5000,
        },
        CallState {
            entry: new,
            sent_ms: NOW - 10,
        },
    ];
    let got = members(&states, NOW, ("", ""));
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].membership_id, "member-a");
    assert_eq!(got[0].focus, Some(focus("https://new.example")));
}

#[test]
fn a_membership_without_created_ts_ages_from_its_origin() {
    let bare = |user: &str| {
        let mut entry = legacy(user, "D", 0, 14_400_000, "https://lk.x.org");
        entry.2.as_object_mut().unwrap().remove("created_ts");
        entry
    };
    let states = [
        CallState {
            entry: bare("@gone:x.org"),
            sent_ms: NOW - 20_000_000,
        },
        CallState {
            entry: bare("@late:x.org"),
            sent_ms: NOW - 10,
        },
        CallState {
            entry: bare("@early:x.org"),
            sent_ms: NOW - 5000,
        },
    ];
    let got = members(&states, NOW, ("", ""));
    let users: Vec<&str> = got.iter().map(|m| m.user.as_str()).collect();
    assert_eq!(users, ["@early:x.org", "@late:x.org"]);
}

#[test]
fn extending_pushes_the_expiry_out() {
    let ours = focus("https://lk.example");
    let mut entry = our_membership("@me:x.org", "DEV", &ours, NOW, false);
    assert_eq!(expires_at(&entry), Some(NOW + 14_400_000));
    extend(&mut entry);
    assert_eq!(entry.2["expires"], 28_800_000);
    assert_eq!(expires_at(&entry), Some(NOW + 28_800_000));
}
