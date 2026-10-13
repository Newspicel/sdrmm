import { describe, expect, it } from "vitest";
import type { PatchEdge, PatchGraph } from "../lib/types";
import { CATALOG } from "../test/catalog";
import { resolvedPatch } from "../test/resolved";
import { toFlowEdges } from "./flowEdges";

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
