import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  authorKey,
  createPointerSender,
  draggedByOthers,
  othersOf,
  POINTER_MS,
  peopleOf,
  usePresenceStore,
} from "./presence";
import type { Pointer } from "./types";

const ann = { id: 1, name: "Ann", hue: 10 };
const bob = { id: 2, name: "Bob", hue: 200 };

describe("presence store", () => {
  beforeEach(() => usePresenceStore.getState().reset());

  it("tracks peers and drops the pointers of those who left", () => {
    const { observe } = usePresenceStore.getState();
    observe({ type: "Peers", data: { you: 1, peers: [ann, bob] } });
    observe({ type: "PeerPointer", data: { peer: 2, pointer: { at: { x: 1, y: 2 } } } });
    expect(usePresenceStore.getState().pointers[2]?.at).toEqual({ x: 1, y: 2 });
    observe({ type: "Peers", data: { you: 1, peers: [ann] } });
    expect(usePresenceStore.getState().pointers).toEqual({});
    expect(othersOf(usePresenceStore.getState().peers, 1)).toEqual([]);
  });

  it("ignores a pointer from someone not listed", () => {
    usePresenceStore.getState().observe({ type: "PeerPointer", data: { peer: 9, pointer: {} } });
    expect(usePresenceStore.getState().pointers).toEqual({});
  });

  it("collects what others are dragging", () => {
    const dragged = draggedByOthers(
      {
        1: { dragging: [{ node: "mine", position: { x: 0, y: 0 } }] },
        2: { dragging: [{ node: "scope", position: { x: 5, y: 6 } }] },
      },
      1,
    );
    expect([...dragged]).toEqual([["scope", { x: 5, y: 6 }]]);
  });
});

describe("pointer sender", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("sends at once, then at most once per interval with the latest", () => {
    const sent: Pointer[] = [];
    const send = createPointerSender((pointer) => sent.push(pointer));
    send({ at: { x: 1, y: 1 } });
    send({ at: { x: 2, y: 2 } });
    send({ at: { x: 3, y: 3 } });
    expect(sent).toEqual([{ at: { x: 1, y: 1 } }]);
    vi.advanceTimersByTime(POINTER_MS);
    expect(sent).toEqual([{ at: { x: 1, y: 1 } }, { at: { x: 3, y: 3 } }]);
  });

  it("skips a repeat of what it last sent", () => {
    const sent: Pointer[] = [];
    const send = createPointerSender((pointer) => sent.push(pointer));
    send({ selected: ["a"] });
    vi.advanceTimersByTime(POINTER_MS);
    send({ selected: ["a"] });
    expect(sent).toHaveLength(1);
  });
});

describe("author key", () => {
  it("stays the same across calls", () => {
    expect(authorKey()).toBe(authorKey());
    expect(authorKey()).toMatch(/^[A-Za-z0-9_-]+$/);
  });
});

describe("people", () => {
  it("folds one person's tabs together and leaves you out of the others", () => {
    const peers = [
      { id: 1, name: "Ann", hue: 10 },
      { id: 2, name: "Ann", hue: 10 },
      { id: 3, name: "Bob", hue: 200 },
      { id: 4, name: "Bob", hue: 200 },
      { id: 5, name: "Cy", hue: 90 },
    ];
    const { me, others } = peopleOf(peers, 1);
    expect(me).toEqual({ name: "Ann", hue: 10, tabs: 2 });
    expect(others).toEqual([
      { name: "Bob", hue: 200, tabs: 2 },
      { name: "Cy", hue: 90, tabs: 1 },
    ]);
  });
});
