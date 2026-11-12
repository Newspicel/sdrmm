import { describe, expect, it } from "vitest";
import type { PatchGraph, PatchNode } from "../lib/types";
import { startsTalking, type TalkPress, talkTargets } from "./pushToTalk";

function press(over: Partial<TalkPress> = {}): TalkPress {
  return { key: " ", repeat: false, ctrlKey: false, metaKey: false, altKey: false, ...over };
}

function node(id: string, kind: string): PatchNode {
  return { id, kind, data: {}, position: { x: 0, y: 0 } } as unknown as PatchNode;
}

describe("startsTalking", () => {
  it("takes the bare key once", () => {
    expect(startsTalking(press(), false)).toBe(true);
    expect(startsTalking(press({ repeat: true }), false)).toBe(false);
    expect(startsTalking(press({ key: "t" }), false)).toBe(false);
    expect(startsTalking(press({ ctrlKey: true }), false)).toBe(false);
    expect(startsTalking(press({ metaKey: true }), false)).toBe(false);
    expect(startsTalking(press({ altKey: true }), false)).toBe(false);
  });

  it("leaves the key to typing and to a control that uses it", () => {
    expect(startsTalking(press(), true)).toBe(false);
  });
});

describe("talkTargets", () => {
  const graph: PatchGraph = {
    nodes: [node("a", "microphone"), node("dev", "device"), node("b", "microphone")],
    edges: [],
  };

  it("keys the selected microphone alone", () => {
    expect(talkTargets(graph, "b")).toEqual(["b"]);
  });

  it("keys every microphone when none is selected", () => {
    expect(talkTargets(graph, null)).toEqual(["a", "b"]);
    expect(talkTargets(graph, "dev")).toEqual(["a", "b"]);
    expect(talkTargets({ nodes: [node("dev", "device")], edges: [] }, null)).toEqual([]);
  });
});
