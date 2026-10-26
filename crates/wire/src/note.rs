use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub const MAX_NOTE_LEN: usize = 4096;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct NoteNode {
    pub text: String,
}

impl NoteNode {
    #[must_use]
    pub fn valid(&self) -> bool {
        self.text.chars().count() <= MAX_NOTE_LEN
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blank_note_is_valid() {
        assert!(NoteNode::default().valid());
    }

    #[test]
    fn a_note_at_the_limit_is_kept() {
        let full = NoteNode {
            text: "ä".repeat(MAX_NOTE_LEN),
        };
        assert!(full.valid());
    }

    #[test]
    fn a_longer_note_is_refused() {
        let over = NoteNode {
            text: "x".repeat(MAX_NOTE_LEN + 1),
        };
        assert!(!over.valid());
    }
}
