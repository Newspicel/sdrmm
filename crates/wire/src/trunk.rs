use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// The protocol a trunk node runs when the operator leaves it to the signalling.
pub const AUTO_TRUNK_PROTOCOL: &str = "auto";

pub const MAX_TRUNK_TALKGROUPS: usize = 256;

/// How a trunked protocol places its calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TrunkKind {
    /// A control channel grants each call a logical channel elsewhere.
    Grants,
    /// Every carrier is a traffic channel and the system is followed by watching all of them.
    Distributed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TrunkScheme {
    pub id: String,
    pub label: String,
    pub kind: TrunkKind,
}

/// How a decoder trunks: the protocols it recognises, how many calls share a carrier and how
/// far its logical channels are numbered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TrunkSpec {
    /// Timeslots per carrier; one on a protocol that gives every call a carrier of its own.
    pub slots: u8,
    /// The highest logical channel number the protocol can name.
    #[serde(default = "widest_channel")]
    pub max_channel: u16,
    pub schemes: Vec<TrunkScheme>,
    /// Followed calls can play through a vocoder chip.
    #[serde(default)]
    pub vocoder: bool,
}

const fn widest_channel() -> u16 {
    u16::MAX
}

impl TrunkSpec {
    #[must_use]
    pub fn scheme(&self, id: &str) -> Option<&TrunkScheme> {
        self.schemes.iter().find(|scheme| scheme.id == id)
    }

    /// The timeslots a carrier holds calls in: none to name on a protocol with one call each.
    #[must_use]
    pub fn timeslots(&self) -> Vec<Option<u8>> {
        if self.slots <= 1 {
            vec![None]
        } else {
            (1..=self.slots).map(Some).collect()
        }
    }
}

/// Which talkgroups a trunk node follows, and which win a traffic channel when all are busy.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct TrunkTalkgroups {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub priority: Vec<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lockout: Vec<u32>,
    /// Follow only the priority talkgroups.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub listed_only: bool,
}

impl TrunkTalkgroups {
    #[must_use]
    pub fn valid(&self) -> bool {
        self.priority.len() <= MAX_TRUNK_TALKGROUPS && self.lockout.len() <= MAX_TRUNK_TALKGROUPS
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.priority.is_empty() && self.lockout.is_empty() && !self.listed_only
    }

    #[must_use]
    pub fn follows(&self, talkgroup: u32) -> bool {
        !self.lockout.contains(&talkgroup) && (!self.listed_only || self.priority(talkgroup))
    }

    #[must_use]
    pub fn priority(&self, talkgroup: u32) -> bool {
        self.priority.contains(&talkgroup)
    }
}

/// The identity a site broadcasts on its control channel, each part as wide as the protocol
/// numbers it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TrunkSite {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_id: Option<u32>,
}

impl TrunkSite {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.system_id.is_none() && self.network_id.is_none() && self.site_id.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_locked_out_talkgroup_is_never_followed() {
        let talkgroups = TrunkTalkgroups {
            priority: vec![9],
            lockout: vec![9, 7],
            listed_only: false,
        };
        assert!(!talkgroups.follows(7));
        assert!(!talkgroups.follows(9));
        assert!(talkgroups.follows(3));
    }

    #[test]
    fn listed_only_follows_just_the_priority_talkgroups() {
        let talkgroups = TrunkTalkgroups {
            priority: vec![9],
            lockout: Vec::new(),
            listed_only: true,
        };
        assert!(talkgroups.follows(9));
        assert!(!talkgroups.follows(3));
    }

    #[test]
    fn a_carrier_with_one_call_names_no_timeslot() {
        let spec = |slots| TrunkSpec {
            slots,
            max_channel: u16::MAX,
            schemes: Vec::new(),
            vocoder: false,
        };
        assert_eq!(spec(1).timeslots(), vec![None]);
        assert_eq!(spec(2).timeslots(), vec![Some(1), Some(2)]);
    }

    #[test]
    fn an_old_node_without_talkgroups_follows_everything() {
        let talkgroups: TrunkTalkgroups = serde_json::from_str("{}").expect("empty rules");
        assert!(talkgroups.follows(1234));
    }
}
