use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU32, Ordering},
    },
};

use sdrmm_wire::{Peer, Pointer, author_hue, peer_name};
use tokio::sync::broadcast;

const PRESENCE_CAP: usize = 256;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PresenceEvent {
    Peers(Arc<Vec<Peer>>),
    Pointer { peer: u32, pointer: Arc<Pointer> },
}

struct Present {
    author: String,
    peer: Peer,
}

pub(crate) struct Presence {
    peers: Mutex<BTreeMap<u32, Present>>,
    next: AtomicU32,
    tx: broadcast::Sender<PresenceEvent>,
}

impl Default for Presence {
    fn default() -> Self {
        Self {
            peers: Mutex::new(BTreeMap::new()),
            next: AtomicU32::new(1),
            tx: broadcast::channel(PRESENCE_CAP).0,
        }
    }
}

impl Presence {
    pub(crate) fn connect(&self) -> u32 {
        self.next.fetch_add(1, Ordering::Relaxed)
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<PresenceEvent> {
        self.tx.subscribe()
    }

    pub(crate) fn present(&self, id: u32, author: &str, wanted: &str, fallback: &str) {
        let mut peers = self.lock();
        let name = peer_name(wanted)
            .or_else(|| peer_name(fallback))
            .or_else(|| guest_of(&peers, id, author))
            .unwrap_or_else(|| format!("Guest {id}"));
        let peer = Peer {
            id,
            name,
            hue: author_hue(author),
        };
        peers.insert(
            id,
            Present {
                author: author.to_owned(),
                peer,
            },
        );
        self.announce(&peers);
    }

    pub(crate) fn point(&self, id: u32, pointer: Pointer) -> bool {
        if !self.lock().contains_key(&id) {
            return false;
        }
        let _ = self.tx.send(PresenceEvent::Pointer {
            peer: id,
            pointer: Arc::new(pointer),
        });
        true
    }

    pub(crate) fn leave(&self, id: u32) {
        let mut peers = self.lock();
        if peers.remove(&id).is_some() {
            self.announce(&peers);
        }
    }

    pub(crate) fn peers(&self) -> Arc<Vec<Peer>> {
        Arc::new(listed(&self.lock()))
    }

    pub(crate) fn name_of(&self, author: &str) -> Option<String> {
        self.lock()
            .values()
            .find(|present| present.author == author)
            .map(|present| present.peer.name.clone())
    }

    fn announce(&self, peers: &BTreeMap<u32, Present>) {
        let _ = self.tx.send(PresenceEvent::Peers(Arc::new(listed(peers))));
    }

    fn lock(&self) -> MutexGuard<'_, BTreeMap<u32, Present>> {
        self.peers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn guest_of(peers: &BTreeMap<u32, Present>, id: u32, author: &str) -> Option<String> {
    peers
        .iter()
        .find(|(other, present)| **other != id && present.author == author)
        .map(|(_, present)| present.peer.name.clone())
}

fn listed(peers: &BTreeMap<u32, Present>) -> Vec<Peer> {
    peers.values().map(|present| present.peer.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joining_and_leaving_announce_the_list() {
        let presence = Presence::default();
        let mut events = presence.subscribe();
        let id = presence.connect();
        presence.present(id, "abc", "  Ann ", "");
        let Ok(PresenceEvent::Peers(peers)) = events.try_recv() else {
            panic!("no list after joining");
        };
        assert_eq!(peers[0].name, "Ann");
        assert_eq!(peers[0].hue, author_hue("abc"));
        assert_eq!(presence.name_of("abc").as_deref(), Some("Ann"));
        presence.leave(id);
        assert!(matches!(events.try_recv(), Ok(PresenceEvent::Peers(peers)) if peers.is_empty()));
    }

    #[test]
    fn an_unnamed_peer_takes_the_fallback() {
        let presence = Presence::default();
        let first = presence.connect();
        let second = presence.connect();
        presence.present(first, "a", "", "relay-user");
        presence.present(second, "b", " ", "");
        let peers = presence.peers();
        assert_eq!(peers[0].name, "relay-user");
        assert_eq!(peers[1].name, format!("Guest {second}"));
    }

    #[test]
    fn a_second_tab_keeps_the_first_tabs_name() {
        let presence = Presence::default();
        let first = presence.connect();
        let second = presence.connect();
        presence.present(first, "same", "", "");
        presence.present(second, "same", "", "");
        let peers = presence.peers();
        assert_eq!(peers[0].name, peers[1].name);
    }

    #[test]
    fn only_present_peers_point() {
        let presence = Presence::default();
        let id = presence.connect();
        assert!(!presence.point(id, Pointer::default()));
        presence.present(id, "a", "Ann", "");
        assert!(presence.point(id, Pointer::default()));
    }

    #[test]
    fn leaving_unannounced_says_nothing() {
        let presence = Presence::default();
        let mut events = presence.subscribe();
        presence.leave(presence.connect());
        assert!(events.try_recv().is_err());
    }
}
