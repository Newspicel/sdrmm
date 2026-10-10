import { useMemo } from "react";
import { create } from "zustand";
import { useShallow } from "zustand/react/shallow";
import { PRESENCE_LIMITS } from "./limits";
import type { Peer, Pointer, Position, ServerEvent } from "./types";

const AUTHOR_KEY = "sdrmm.author";
const NAME_KEY = "sdrmm.name";
const POINTER_HEADROOM = 1.5;
export const POINTER_MS = Math.ceil((1000 * POINTER_HEADROOM) / PRESENCE_LIMITS.pointer_rate_hz);

let fallbackAuthor: string | null = null;

function freshAuthor(): string {
  return crypto.randomUUID();
}

export function authorKey(): string {
  try {
    const held = localStorage.getItem(AUTHOR_KEY);
    if (held !== null && held !== "") {
      return held;
    }
    const made = freshAuthor();
    localStorage.setItem(AUTHOR_KEY, made);
    return made;
  } catch {
    fallbackAuthor ??= freshAuthor();
    return fallbackAuthor;
  }
}

export function savedName(): string {
  try {
    return localStorage.getItem(NAME_KEY) ?? "";
  } catch {
    return "";
  }
}

export function saveName(name: string): void {
  try {
    localStorage.setItem(NAME_KEY, name.trim());
  } catch {
    return;
  }
}

export function peerColor(peer: Pick<Peer, "hue">): string {
  return `hsl(${peer.hue} 75% 55%)`;
}

export function initialOf(name: string): string {
  return name.trim().charAt(0).toUpperCase() || "?";
}

export interface PresenceState {
  you: number | null;
  peers: readonly Peer[];
  pointers: Readonly<Record<number, Pointer>>;
  observe: (event: ServerEvent) => void;
  reset: () => void;
}

export const usePresenceStore = create<PresenceState>((set) => ({
  you: null,
  peers: [],
  pointers: {},
  observe: (event: ServerEvent) => {
    if (event.type === "Peers") {
      const live = new Set(event.data.peers.map((peer) => peer.id));
      set((state) => ({
        you: event.data.you,
        peers: event.data.peers,
        pointers: Object.fromEntries(
          Object.entries(state.pointers).filter(([peer]) => live.has(Number(peer))),
        ),
      }));
    } else if (event.type === "PeerPointer") {
      set((state) =>
        state.peers.some((peer) => peer.id === event.data.peer)
          ? { pointers: { ...state.pointers, [event.data.peer]: event.data.pointer } }
          : state,
      );
    }
  },
  reset: () => set({ you: null, peers: [], pointers: {} }),
}));

export function othersOf(peers: readonly Peer[], you: number | null): Peer[] {
  return peers.filter((peer) => peer.id !== you);
}

export function useOthers(): Peer[] {
  return usePresenceStore(useShallow((state) => othersOf(state.peers, state.you)));
}

export interface Person {
  name: string;
  hue: number;
  tabs: number;
}

export interface People {
  me: Person | null;
  others: Person[];
}

function personKey(peer: Pick<Peer, "name" | "hue">): string {
  return `${peer.hue}:${peer.name}`;
}

export function peopleOf(peers: readonly Peer[], you: number | null): People {
  const mine = peers.find((peer) => peer.id === you);
  const grouped = new Map<string, Person>();
  for (const peer of peers) {
    const key = personKey(peer);
    const held = grouped.get(key);
    grouped.set(key, { name: peer.name, hue: peer.hue, tabs: (held?.tabs ?? 0) + 1 });
  }
  const meKey = mine === undefined ? null : personKey(mine);
  return {
    me: meKey === null ? null : (grouped.get(meKey) ?? null),
    others: [...grouped.entries()].filter(([key]) => key !== meKey).map(([, person]) => person),
  };
}

export function usePeople(): People {
  const peers = usePresenceStore((state) => state.peers);
  const you = usePresenceStore((state) => state.you);
  return useMemo(() => peopleOf(peers, you), [peers, you]);
}

export function samePointer(a: Pointer, b: Pointer): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

export function createPointerSender(
  send: (pointer: Pointer) => void,
  every = POINTER_MS,
): (pointer: Pointer) => void {
  let last: Pointer | null = null;
  let queued: Pointer | null = null;
  let timer: ReturnType<typeof setTimeout> | null = null;
  const flush = () => {
    timer = null;
    if (queued === null) {
      return;
    }
    const next = queued;
    queued = null;
    if (last !== null && samePointer(last, next)) {
      return;
    }
    last = next;
    send(next);
    timer = setTimeout(flush, every);
  };
  return (pointer: Pointer) => {
    queued = pointer;
    if (timer === null) {
      flush();
    }
  };
}

export function draggedByOthers(
  pointers: Readonly<Record<number, Pointer>>,
  you: number | null,
): ReadonlyMap<string, Position> {
  const dragged = new Map<string, Position>();
  for (const [peer, pointer] of Object.entries(pointers)) {
    if (Number(peer) === you) {
      continue;
    }
    for (const node of pointer.dragging ?? []) {
      dragged.set(node.node, node.position);
    }
  }
  return dragged;
}
