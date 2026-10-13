import { describe, expect, it } from "vitest";
import type { PatchEdge, PatchGraph } from "../lib/types";
import { CATALOG } from "../test/catalog";
import { resolvedPatch } from "../test/resolved";
import { RETUNES_TITLE, toFlowEdges } from "./flowEdges";

const wire = (from: string, to: string): PatchEdge => ({
  from: { node: from, port: "iq" },
  to: { node: to, port: "iq" },
});

const GRAPH: PatchGraph = {
  nodes: [],
  edges: [wire("radio", "voice"), wire("radio", "scope")],
};

describe("toFlowEdges", () => {
  it("marks only the wires the server says retune their radio", () => {
    const resolved = {
      ...resolvedPatch([]),
      edges: [
        { ...wire("radio", "voice"), retunes_source: true },
        { ...wire("radio", "scope"), retunes_source: false },
      ],
    };
    const [voice, scope] = toFlowEdges(GRAPH, { catalog: CATALOG, channelTypes: [], resolved });
    expect(voice?.className).toContain("wire-retunes");
    expect(voice?.markerStart).toBeDefined();
    expect(voice?.ariaLabel).toBe(RETUNES_TITLE);
    expect(scope?.className).not.toContain("wire-retunes");
    expect(scope?.markerStart).toBeUndefined();
  });
});
