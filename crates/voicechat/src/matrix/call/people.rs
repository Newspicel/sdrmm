//! Who is in the call, which `LiveKit` identities are theirs, and whose key came how.

use std::collections::HashMap;

use crate::Talker;
use crate::matrix::e2ee::keys::EncryptionKeys;
use crate::matrix::membership::{CallState, Member, identities, members};

/// Our own user and device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Me {
    pub(crate) user: String,
    pub(crate) device: String,
}

impl Me {
    /// The membership id we publish and claim in key events.
    pub(crate) fn member_id(&self) -> String {
        format!("{}:{}", self.user, self.device)
    }
}

/// Members that came and went since the last look.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Change {
    pub(crate) joined: Vec<Member>,
    pub(crate) left: Vec<Member>,
}

impl Change {
    pub(crate) fn is_empty(&self) -> bool {
        self.joined.is_empty() && self.left.is_empty()
    }
}

type StateKey = (String, String);

pub(crate) struct People {
    me: Me,
    room: String,
    encrypted: bool,
    states: HashMap<StateKey, CallState>,
    members: Vec<Member>,
    by_identity: HashMap<String, usize>,
    /// The `(user, device)` each identity's key last came from by Olm.
    olm: HashMap<String, (String, String)>,
    names: HashMap<String, String>,
}

fn same(a: &Member, b: &Member) -> bool {
    a.user == b.user && a.device == b.device && a.membership_id == b.membership_id
}

fn missing_from(these: &[Member], those: &[Member]) -> Vec<Member> {
    these
        .iter()
        .filter(|member| !those.iter().any(|other| same(member, other)))
        .cloned()
        .collect()
}

impl People {
    pub(crate) fn new(me: Me, room: String, encrypted: bool) -> Self {
        Self {
            me,
            room,
            encrypted,
            states: HashMap::new(),
            members: Vec::new(),
            by_identity: HashMap::new(),
            olm: HashMap::new(),
            names: HashMap::new(),
        }
    }

    pub(crate) fn me(&self) -> &Me {
        &self.me
    }

    pub(crate) fn room(&self) -> &str {
        &self.room
    }

    pub(crate) fn encrypted(&self) -> bool {
        self.encrypted
    }

    pub(crate) fn members(&self) -> &[Member] {
        &self.members
    }

    /// Takes the room's call state as a whole.
    pub(crate) fn seed(&mut self, states: Vec<CallState>, now_ms: u64) -> Change {
        self.states = states
            .into_iter()
            .map(|state| ((state.entry.0.clone(), state.entry.1.clone()), state))
            .collect();
        self.recount(now_ms)
    }

    pub(crate) fn update(&mut self, state: CallState, now_ms: u64) -> Change {
        let key = (state.entry.0.clone(), state.entry.1.clone());
        self.states.insert(key, state);
        self.recount(now_ms)
    }

    /// Reads the members again, so expired ones leave.
    pub(crate) fn recount(&mut self, now_ms: u64) -> Change {
        let states: Vec<CallState> = self.states.values().cloned().collect();
        let now = members(&states, now_ms, (&self.me.user, &self.me.device));
        let change = Change {
            joined: missing_from(&now, &self.members),
            left: missing_from(&self.members, &now),
        };
        self.by_identity = now
            .iter()
            .enumerate()
            .flat_map(|(at, member)| identities(member).into_iter().map(move |id| (id, at)))
            .collect();
        self.members = now;
        change
    }

    pub(crate) fn named(&mut self, user: String, name: String) {
        self.names.insert(user, name);
    }

    /// The identities a received key is for. A key sent in the clear or for another room is
    /// ignored, as js-sdk `ToDeviceKeyTransport` does.
    pub(crate) fn key_from(
        &mut self,
        sender: &str,
        device: Option<&str>,
        content: &EncryptionKeys,
    ) -> Vec<String> {
        let Some(device) = device else {
            tracing::warn!(sender, "media key sent in the clear ignored");
            return Vec::new();
        };
        if content.room_id != self.room {
            return Vec::new();
        }
        let claimed = &content.member.claimed_device_id;
        let sent_by = Member {
            user: sender.to_owned(),
            device: claimed.clone(),
            membership_id: content.sender_member(sender),
            focus: None,
        };
        let ids = identities(&sent_by);
        for id in &ids {
            if device == claimed {
                self.olm
                    .insert(id.clone(), (sender.to_owned(), device.to_owned()));
            } else {
                self.olm.remove(id);
            }
        }
        ids
    }

    /// A member's identity is verified once in the room's membership and, when encrypted,
    /// its key came by Olm from that member's device. Anyone else is a stand-in.
    pub(crate) fn talker(&self, identity: &str) -> Talker {
        let Some(member) = self.by_identity.get(identity).map(|&at| &self.members[at]) else {
            return Talker {
                id: identity.to_owned(),
                name: identity.to_owned(),
                verified: false,
            };
        };
        let keyed = self
            .olm
            .get(identity)
            .is_some_and(|(user, device)| *user == member.user && *device == member.device);
        Talker {
            id: member.user.clone(),
            name: self
                .names
                .get(&member.user)
                .cloned()
                .unwrap_or_else(|| member.user.clone()),
            verified: !self.encrypted || keyed,
        }
    }
}
