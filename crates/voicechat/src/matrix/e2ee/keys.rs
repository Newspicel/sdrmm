//! Media keys of a `MatrixRTC` call: ours, theirs and the to-device event that carries them.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use serde::{Deserialize, Serialize};

use super::frame::{CryptoError, FrameKey, KeyRing};

#[cfg_attr(not(test), expect(dead_code, reason = "wired in Task 9"))]
pub(crate) const KEYS_EVENT: &str = "io.element.call.encryption_keys";
#[cfg_attr(not(test), expect(dead_code, reason = "wired in Task 9"))]
pub(crate) const KEYS_EVENT_STABLE: &str = "m.rtc.encryption_key";
const KEY_LEN: usize = 16;
const USE_DELAY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct KeyEntry {
    pub(crate) index: u8,
    pub(crate) key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MemberClaim {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) id: Option<String>,
    pub(crate) claimed_device_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SessionClaim {
    pub(crate) application: String,
    pub(crate) call_id: String,
    pub(crate) scope: String,
}

/// Content of the to-device key event. js-sdk sends the key as both `media_key` and the
/// deprecated `keys`, and reads either; older senders add `session` and `sent_ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EncryptionKeys {
    pub(crate) room_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) member_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) media_key: Option<KeyEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) keys: Option<KeyEntry>,
    pub(crate) member: MemberClaim,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) session: Option<SessionClaim>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) sent_ts: Option<u64>,
}

impl EncryptionKeys {
    pub(crate) fn new(room_id: &str, member_id: &str, device_id: &str, out: &Outgoing) -> Self {
        let entry = KeyEntry {
            index: out.index,
            key: STANDARD_NO_PAD.encode(out.material),
        };
        Self {
            room_id: room_id.to_owned(),
            member_id: Some(member_id.to_owned()),
            media_key: Some(entry.clone()),
            keys: Some(entry),
            member: MemberClaim {
                id: Some(member_id.to_owned()),
                claimed_device_id: device_id.to_owned(),
            },
            session: None,
            sent_ts: None,
        }
    }

    /// The key as sent, preferring `media_key`; None when absent or not base64.
    pub(crate) fn entry(&self) -> Option<(u8, Vec<u8>)> {
        let entry = self.media_key.as_ref().or(self.keys.as_ref())?;
        let text = entry.key.trim_end_matches('=');
        let bytes = STANDARD_NO_PAD
            .decode(text)
            .or_else(|_| STANDARD.decode(&entry.key))
            .ok()?;
        Some((entry.index, bytes))
    }

    pub(crate) fn sender_member(&self, user_id: &str) -> String {
        self.member_id
            .clone()
            .or_else(|| self.member.id.clone())
            .unwrap_or_else(|| format!("{user_id}:{}", self.member.claimed_device_id))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Outgoing {
    pub(crate) index: u8,
    pub(crate) material: [u8; KEY_LEN],
}

struct OwnKey {
    outgoing: Outgoing,
    key: FrameKey,
    from: Instant,
}

impl OwnKey {
    fn new(index: u8, from: Instant) -> Result<Self, CryptoError> {
        let mut material = [0u8; KEY_LEN];
        getrandom::fill(&mut material).map_err(|_| CryptoError::Derive)?;
        Ok(Self {
            key: FrameKey::derive(&material)?,
            outgoing: Outgoing { index, material },
            from,
        })
    }
}

/// Our send key, the keys others sent us, and the rotation between them.
pub(crate) struct MediaKeys {
    current: OwnKey,
    next: Option<OwnKey>,
    theirs: HashMap<String, KeyRing>,
}

impl MediaKeys {
    pub(crate) fn new(now: Instant) -> Result<Self, CryptoError> {
        Ok(Self {
            current: OwnKey::new(0, now)?,
            next: None,
            theirs: HashMap::new(),
        })
    }

    /// The key to encrypt with at `now`; a rotated key waits out its use delay.
    pub(crate) fn ours(&self, now: Instant) -> (u8, &FrameKey) {
        let own = match &self.next {
            Some(next) if next.from <= now => next,
            _ => &self.current,
        };
        (own.outgoing.index, &own.key)
    }

    /// Rotates to a new key at the next index, used from `now` plus the use delay.
    pub(crate) fn on_left(&mut self, now: Instant) -> Result<Outgoing, CryptoError> {
        let latest = self.latest();
        let index = latest.outgoing.index.wrapping_add(1);
        let fresh = OwnKey::new(index, now + USE_DELAY)?;
        let out = fresh.outgoing.clone();
        if let Some(due) = self.next.take() {
            self.current = due;
        }
        self.next = Some(fresh);
        Ok(out)
    }

    /// The newest key, for a participant that just joined.
    pub(crate) fn on_joined(&self) -> Outgoing {
        self.latest().outgoing.clone()
    }

    fn latest(&self) -> &OwnKey {
        self.next.as_ref().unwrap_or(&self.current)
    }

    pub(crate) fn receive(
        &mut self,
        from: &str,
        index: u8,
        material: &[u8],
    ) -> Result<(), CryptoError> {
        let key = FrameKey::derive(material)?;
        self.theirs
            .entry(from.to_owned())
            .or_insert_with(KeyRing::new)
            .set(index, key);
        Ok(())
    }

    pub(crate) fn ring(&self, identity: &str) -> Option<&KeyRing> {
        self.theirs.get(identity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JS_EVENT: &str = r#"{
        "room_id": "!room:example.org",
        "member_id": "abc123",
        "media_key": {"index": 3, "key": "AAECAwQFBgcICQoLDA0ODw"},
        "keys": {"index": 3, "key": "AAECAwQFBgcICQoLDA0ODw"},
        "member": {"claimed_device_id": "DEVICE", "id": "abc123"}
    }"#;

    const OLD_EVENT: &str = r#"{
        "keys": {"index": 1, "key": "AAECAwQFBgcICQoLDA0ODw"},
        "member": {"id": "@a:example.org:DEVICE", "claimed_device_id": "DEVICE"},
        "room_id": "!room:example.org",
        "session": {"application": "m.call", "call_id": "", "scope": "m.room"},
        "sent_ts": 1700000000000
    }"#;

    fn sample() -> Vec<u8> {
        (0u8..16).collect()
    }

    #[test]
    fn the_event_types_are_those_of_js_sdk() {
        assert_eq!(KEYS_EVENT, "io.element.call.encryption_keys");
        assert_eq!(KEYS_EVENT_STABLE, "m.rtc.encryption_key");
    }

    #[test]
    fn a_js_sdk_key_event_parses() {
        let event: EncryptionKeys = serde_json::from_str(JS_EVENT).unwrap();
        assert_eq!(event.entry(), Some((3, sample())));
        assert_eq!(event.sender_member("@a:example.org"), "abc123");
        assert_eq!(event.member.claimed_device_id, "DEVICE");

        let old: EncryptionKeys = serde_json::from_str(OLD_EVENT).unwrap();
        assert_eq!(old.entry(), Some((1, sample())));
        assert_eq!(old.sent_ts, Some(1_700_000_000_000));
        assert_eq!(old.sender_member("@a:example.org"), "@a:example.org:DEVICE");
    }

    #[test]
    fn a_sent_event_reads_back() {
        let out = Outgoing {
            index: 9,
            material: [7; 16],
        };
        let event = EncryptionKeys::new("!r:x", "m", "D", &out);
        let json = serde_json::to_string(&event).unwrap();
        let back: EncryptionKeys = serde_json::from_str(&json).unwrap();
        assert_eq!(back.entry(), Some((9, vec![7; 16])));
        assert!(!json.contains('='));
    }

    #[test]
    fn leaving_rotates_after_grace() {
        let t0 = Instant::now();
        let mut keys = MediaKeys::new(t0).unwrap();
        let out = keys.on_left(t0).unwrap();
        assert_eq!(out.index, 1);
        assert_eq!(keys.ours(t0 + Duration::from_secs(4)).0, 0);
        assert_eq!(keys.ours(t0 + Duration::from_secs(5)).0, 1);
        assert_eq!(keys.on_joined(), out);
    }

    #[test]
    fn joining_resends_the_current_key() {
        let t0 = Instant::now();
        let keys = MediaKeys::new(t0).unwrap();
        assert_eq!(keys.on_joined(), keys.on_joined());
        assert_eq!(keys.on_joined().index, 0);
    }

    #[test]
    fn received_keys_land_in_the_senders_ring() {
        let mut keys = MediaKeys::new(Instant::now()).unwrap();
        keys.receive("@a:x:D", 4, &sample()).unwrap();
        let ring = keys.ring("@a:x:D").unwrap();
        assert!(ring.get(4).is_some());
        assert!(ring.get(5).is_none());
        assert!(keys.ring("@b:x:D").is_none());
    }

    #[test]
    fn index_wraps_after_255() {
        let t0 = Instant::now();
        let mut keys = MediaKeys::new(t0).unwrap();
        let mut last = 0;
        for _ in 0..256 {
            last = keys.on_left(t0).unwrap().index;
        }
        assert_eq!(last, 0);
        assert_eq!(keys.on_left(t0).unwrap().index, 1);
    }
}
