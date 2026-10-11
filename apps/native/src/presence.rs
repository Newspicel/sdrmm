use std::collections::HashMap;

use sdrmm_wire::{
    patch::Position,
    presence::{DraggedNode, MAX_POINTER_NODES, POINTER_RATE_HZ, Peer, Pointer},
    ws::ServerEvent,
};

const POINTER_HEADROOM: f64 = 1.5;

#[must_use]
pub fn pointer_ms() -> u64 {
    (1000.0 * POINTER_HEADROOM / f64::from(POINTER_RATE_HZ)).ceil() as u64
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Room {
    pub you: Option<u32>,
    pub peers: Vec<Peer>,
    pub pointers: HashMap<u32, Pointer>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Person {
    pub name: String,
    pub hue: u16,
    pub tabs: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct People {
    pub me: Option<Person>,
    pub others: Vec<Person>,
}

impl Room {
    pub fn observe(&mut self, event: &ServerEvent) -> bool {
        match event {
            ServerEvent::Peers { you, peers } => {
                self.you = Some(*you);
                self.peers.clone_from(peers);
                self.pointers
                    .retain(|peer, _| peers.iter().any(|live| live.id == *peer));
                true
            }
            ServerEvent::PeerPointer { peer, pointer } => {
                if !self.peers.iter().any(|live| live.id == *peer) {
                    return false;
                }
                self.pointers.insert(*peer, pointer.clone());
                true
            }
            _ => false,
        }
    }

    #[must_use]
    pub fn others(&self) -> Vec<Peer> {
        self.peers
            .iter()
            .filter(|peer| Some(peer.id) != self.you)
            .cloned()
            .collect()
    }

    #[must_use]
    pub fn people(&self) -> People {
        let mine = self
            .peers
            .iter()
            .find(|peer| Some(peer.id) == self.you)
            .map(|peer| (peer.hue, peer.name.as_str()));
        let mut grouped: Vec<Person> = Vec::new();
        for peer in &self.peers {
            match grouped
                .iter_mut()
                .find(|person| person.hue == peer.hue && person.name == peer.name)
            {
                Some(person) => person.tabs += 1,
                None => grouped.push(Person {
                    name: peer.name.clone(),
                    hue: peer.hue,
                    tabs: 1,
                }),
            }
        }
        let is_me = |person: &Person| mine == Some((person.hue, person.name.as_str()));
        People {
            me: grouped.iter().find(|person| is_me(person)).cloned(),
            others: grouped
                .iter()
                .filter(|person| !is_me(person))
                .cloned()
                .collect(),
        }
    }

    #[must_use]
    pub fn dragged_by_others(&self) -> HashMap<String, Position> {
        let mut dragged = HashMap::new();
        for (peer, pointer) in &self.pointers {
            if Some(*peer) == self.you {
                continue;
            }
            for node in &pointer.dragging {
                dragged.insert(node.node.clone(), node.position);
            }
        }
        dragged
    }
}

const SATURATION: f64 = 0.75;
const LIGHTNESS: f64 = 0.55;

#[must_use]
pub fn peer_color(hue: u16) -> String {
    let reach = SATURATION * LIGHTNESS.min(1.0 - LIGHTNESS);
    let channel = |n: f64| {
        let k = (n + f64::from(hue % 360) / 30.0) % 12.0;
        let level = LIGHTNESS - reach * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0);
        (level * 255.0).round().clamp(0.0, 255.0) as u8
    };
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(0.0),
        channel(8.0),
        channel(4.0)
    )
}

#[must_use]
pub fn initial_of(name: &str) -> String {
    name.trim()
        .chars()
        .next()
        .map_or_else(|| String::from("?"), |first| first.to_uppercase().collect())
}

#[must_use]
pub fn fitted(mut pointer: Pointer) -> Pointer {
    pointer.selected.truncate(MAX_POINTER_NODES);
    pointer.dragging.truncate(MAX_POINTER_NODES);
    pointer
        .selected
        .retain(|node| node.len() <= sdrmm_wire::patch::MAX_NODE_ID_LEN);
    pointer
        .dragging
        .retain(|node: &DraggedNode| node.node.len() <= sdrmm_wire::patch::MAX_NODE_ID_LEN);
    pointer
}

#[derive(Clone, Debug, Default)]
pub struct PointerGate {
    last: Option<Pointer>,
    queued: Option<Pointer>,
    sent_at: Option<u64>,
}

impl PointerGate {
    pub fn offer(&mut self, pointer: Pointer, now_ms: u64) -> Option<Pointer> {
        self.queued = Some(pointer);
        self.flush(now_ms)
    }

    pub fn flush(&mut self, now_ms: u64) -> Option<Pointer> {
        if self
            .sent_at
            .is_some_and(|at| now_ms < at.saturating_add(pointer_ms()))
        {
            return None;
        }
        let next = self.queued.take()?;
        if self.last.as_ref() == Some(&next) {
            return None;
        }
        self.last = Some(next.clone());
        self.sent_at = Some(now_ms);
        Some(next)
    }

    pub fn forget(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(id: u32, name: &str, hue: u16) -> Peer {
        Peer {
            id,
            name: name.to_owned(),
            hue,
        }
    }

    fn at(x: f32) -> Pointer {
        Pointer {
            at: Some(Position { x, y: 0.0 }),
            ..Pointer::default()
        }
    }

    fn room() -> Room {
        let mut room = Room::default();
        room.observe(&ServerEvent::Peers {
            you: 1,
            peers: vec![peer(1, "Ann", 10), peer(2, "Bob", 20), peer(3, "Ann", 10)],
        });
        room
    }

    #[test]
    fn a_pointer_from_a_stranger_is_ignored() {
        let mut room = room();
        assert!(!room.observe(&ServerEvent::PeerPointer {
            peer: 9,
            pointer: at(1.0),
        }));
        assert!(room.observe(&ServerEvent::PeerPointer {
            peer: 2,
            pointer: at(1.0),
        }));
        assert_eq!(room.pointers.len(), 1);
    }

    #[test]
    fn a_peer_who_leaves_takes_their_pointer() {
        let mut room = room();
        room.observe(&ServerEvent::PeerPointer {
            peer: 2,
            pointer: at(1.0),
        });
        room.observe(&ServerEvent::Peers {
            you: 1,
            peers: vec![peer(1, "Ann", 10)],
        });
        assert!(room.pointers.is_empty());
        assert!(room.others().is_empty());
    }

    #[test]
    fn people_fold_tabs_of_one_person_and_set_me_apart() {
        let people = room().people();
        assert_eq!(
            people.me,
            Some(Person {
                name: "Ann".to_owned(),
                hue: 10,
                tabs: 2
            })
        );
        assert_eq!(people.others.len(), 1);
        assert_eq!(people.others[0].name, "Bob");
    }

    #[test]
    fn only_other_peers_drag_nodes() {
        let mut room = room();
        let dragging = |node: &str, x: f32| Pointer {
            dragging: vec![DraggedNode {
                node: node.to_owned(),
                position: Position { x, y: 2.0 },
            }],
            ..Pointer::default()
        };
        room.observe(&ServerEvent::PeerPointer {
            peer: 1,
            pointer: dragging("mine", 1.0),
        });
        room.observe(&ServerEvent::PeerPointer {
            peer: 2,
            pointer: dragging("theirs", 5.0),
        });
        let dragged = room.dragged_by_others();
        assert_eq!(dragged.len(), 1);
        assert_eq!(dragged["theirs"], Position { x: 5.0, y: 2.0 });
    }

    #[test]
    fn initials_and_colours() {
        assert_eq!(initial_of("  ann"), "A");
        assert_eq!(initial_of(""), "?");
        assert_eq!(peer_color(0), "#e23636");
        assert_eq!(peer_color(120), "#36e236");
        assert_eq!(peer_color(360), peer_color(0));
    }

    #[test]
    fn the_gate_sends_at_once_then_waits_and_skips_repeats() {
        let mut gate = PointerGate::default();
        assert_eq!(gate.offer(at(1.0), 0), Some(at(1.0)));
        assert_eq!(gate.offer(at(2.0), 10), None);
        assert_eq!(gate.offer(at(3.0), 20), None);
        assert_eq!(gate.flush(pointer_ms()), Some(at(3.0)));
        assert_eq!(gate.offer(at(3.0), pointer_ms() * 3), None);
        assert_eq!(gate.flush(pointer_ms() * 4), None);
    }

    #[test]
    fn the_pointer_rate_stays_under_the_server_limit() {
        assert!(1000 / pointer_ms() < u64::from(POINTER_RATE_HZ));
    }

    #[test]
    fn an_oversized_pointer_is_cut_to_fit() {
        let pointer = fitted(Pointer {
            selected: vec!["n".to_owned(); MAX_POINTER_NODES + 4],
            ..Pointer::default()
        });
        assert!(pointer.fits());
    }
}
