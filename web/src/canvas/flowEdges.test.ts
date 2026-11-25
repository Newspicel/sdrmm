import { describe, expect, it } from "vitest";
import type { PatchEdge, PatchGraph } from "../lib/types";
import { CATALOG } from "../test/catalog";
import { resolvedPatch } from "../test/resolved";
import { keptEdges, toFlowEdges } from "./flowEdges";

const wire = (from: string, to: string): PatchEdge => ({
  from: { node: from, port: "iq" },
  to: { node: to, port: "iq" },
});

const GRAPH: PatchGraph = {
  nodes: [],
  edges: [wire("radio", "voice"), wire("radio", "scope")],
};

describe("toFlowEdges", () => {
  it("draws every wire plain, even one that retunes its radio", () => {
    const resolved = {
      ...resolvedPatch([]),
      edges: [
        { ...wire("radio", "voice"), retunes_source: true },
        { ...wire("radio", "scope"), retunes_source: false },
      ],
    };
    const [voice, scope] = toFlowEdges(GRAPH, { catalog: CATALOG, channelTypes: [], resolved });
    for (const edge of [voice, scope]) {
      expect(edge?.markerStart).toBeUndefined();
      expect(edge?.markerEnd).toBeUndefined();
      expect(edge?.className).not.toContain("retunes");
    }
  });
});

function flowEdge(id: string, className: string) {
  return { id, source: "a", target: "b", className };
}

describe("keptEdges", () => {
  it("hands back the same array when no wire changed", () => {
    const previous = [flowEdge("one", "wire-iq"), flowEdge("two", "wire-audio")];
    expect(keptEdges(previous, [flowEdge("one", "wire-iq"), flowEdge("two", "wire-audio")])).toBe(previous);
  });

  it("keeps unchanged wires and swaps the changed one", () => {
    const previous = [flowEdge("one", "wire-iq"), flowEdge("two", "wire-audio")];
    const next = keptEdges(previous, [flowEdge("one", "wire-iq"), flowEdge("two", "wire-events")]);
    expect(next[0]).toBe(previous[0]);
    expect(next[1]).not.toBe(previous[1]);
    expect(next[1]?.className).toBe("wire-events");
  });

  it("keeps a selected wire selected", () => {
    const previous = [{ ...flowEdge("one", "wire-iq"), selected: true }];
    expect(keptEdges(previous, [flowEdge("one", "wire-iq")])[0]?.selected).toBe(true);
  });
});
