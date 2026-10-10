use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::patch::{MAX_NODE_ID_LEN, Position};

pub const AUTHOR_HEADER: &str = "x-sdrmm-author";
pub const MAX_AUTHOR_LEN: usize = 64;
pub const MAX_PEER_NAME_LEN: usize = 32;
pub const MAX_POINTER_NODES: usize = 64;
pub const POINTER_RATE_HZ: u32 = 30;
pub const POINTER_BURST: u32 = 10;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Peer {
    pub id: u32,
    pub name: String,
    pub hue: u16,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DraggedNode {
    pub node: String,
    pub position: Position,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Pointer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Position>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dragging: Vec<DraggedNode>,
}

impl Pointer {
    #[must_use]
    pub fn fits(&self) -> bool {
        self.selected.len() <= MAX_POINTER_NODES
            && self.dragging.len() <= MAX_POINTER_NODES
            && self
                .selected
                .iter()
                .chain(self.dragging.iter().map(|dragged| &dragged.node))
                .all(|node| node.len() <= MAX_NODE_ID_LEN)
    }
}

#[must_use]
pub fn valid_author(author: &str) -> bool {
    !author.is_empty()
        && author.len() <= MAX_AUTHOR_LEN
        && author
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

#[must_use]
pub fn peer_name(wanted: &str) -> Option<String> {
    let trimmed = wanted.trim();
    (!trimmed.is_empty()).then(|| trimmed.chars().take(MAX_PEER_NAME_LEN).collect())
}

#[must_use]
pub fn author_hue(author: &str) -> u16 {
    let hash = author.bytes().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    });
    u16::try_from(hash % 360).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authors_are_short_url_safe_tokens() {
        assert!(valid_author("3f2a-b_9"));
        assert!(!valid_author(""));
        assert!(!valid_author("a b"));
        assert!(!valid_author(&"a".repeat(MAX_AUTHOR_LEN + 1)));
    }

    #[test]
    fn names_are_trimmed_and_capped() {
        assert_eq!(peer_name("  Ann "), Some("Ann".to_owned()));
        assert_eq!(peer_name("   "), None);
        assert_eq!(
            peer_name(&"x".repeat(80)).map(|name| name.chars().count()),
            Some(MAX_PEER_NAME_LEN)
        );
    }

    #[test]
    fn hue_is_stable_per_author() {
        assert_eq!(author_hue("abc"), author_hue("abc"));
        assert!(author_hue("abc") < 360);
    }

    #[test]
    fn pointer_refuses_oversized_selections() {
        let mut pointer = Pointer {
            selected: vec!["n".to_owned(); MAX_POINTER_NODES],
            ..Pointer::default()
        };
        assert!(pointer.fits());
        pointer.selected.push("n".to_owned());
        assert!(!pointer.fits());
    }
}
