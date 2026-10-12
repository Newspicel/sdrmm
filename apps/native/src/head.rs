use std::{collections::HashMap, sync::Arc};

use zgui::{prelude::*, reactive::RenderEffect};

use crate::store::Store;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Plain,
    Warn,
    Danger,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NodeHead {
    pub subtitle: Option<String>,
    pub tone: Tone,
    pub badge: Option<String>,
    pub title: Option<String>,
}

impl NodeHead {
    #[must_use]
    pub fn said(subtitle: impl Into<String>) -> Self {
        Self {
            subtitle: Some(subtitle.into()),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn toned(self, tone: Tone) -> Self {
        Self { tone, ..self }
    }

    #[must_use]
    pub fn titled(self, title: impl Into<String>) -> Self {
        Self {
            title: Some(title.into()),
            ..self
        }
    }

    #[must_use]
    pub fn badged(self, badge: impl Into<String>) -> Self {
        Self {
            badge: Some(badge.into()),
            ..self
        }
    }
}

pub type Heads = Arc<HashMap<String, NodeHead>>;

#[must_use]
pub fn with_head(heads: &Heads, node: &str, head: NodeHead) -> Option<Heads> {
    let empty = head == NodeHead::default();
    if heads.get(node) == Some(&head) || (empty && !heads.contains_key(node)) {
        return None;
    }
    let mut next = (**heads).clone();
    if empty {
        next.remove(node);
    } else {
        next.insert(node.to_owned(), head);
    }
    Some(Arc::new(next))
}

pub fn show(store: Store, node: String, read: impl Fn() -> NodeHead + 'static) {
    let shown = node.clone();
    let effect = RenderEffect::new(move |_| {
        let head = read();
        let current = store.heads.get_untracked();
        if let Some(next) = with_head(&current, &shown, head) {
            store.heads.set(next);
        }
    });
    on_cleanup_local(move || {
        drop(effect);
        let current = store.heads.get_untracked();
        if let Some(next) = with_head(&current, &node, NodeHead::default()) {
            store.heads.set(next);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_head_is_kept_until_it_is_cleared() {
        let heads: Heads = Arc::default();
        let said = with_head(
            &heads,
            "scope",
            NodeHead::said("not open").toned(Tone::Warn),
        )
        .expect("a change");
        assert_eq!(said["scope"].tone, Tone::Warn);
        assert!(with_head(&said, "scope", NodeHead::said("not open").toned(Tone::Warn)).is_none());
        let cleared = with_head(&said, "scope", NodeHead::default()).expect("cleared");
        assert!(cleared.is_empty());
        assert!(with_head(&cleared, "scope", NodeHead::default()).is_none());
    }

    #[test]
    fn a_badge_rides_beside_the_subtitle() {
        let head = NodeHead::said("2 lanes").badged("FM");
        assert_eq!(head.badge.as_deref(), Some("FM"));
        assert_eq!(head.subtitle.as_deref(), Some("2 lanes"));
        assert_eq!(
            head.clone().titled("3 of 5").title.as_deref(),
            Some("3 of 5")
        );
    }
}
