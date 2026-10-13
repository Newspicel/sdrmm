import { describe, expect, it, vi } from "vitest";
import { useProcessorStore } from "../lib/processors";
import type { PatchGraph, PatchNode, WorkspaceSnapshot } from "../lib/types";
import { dropNodes } from "./remove";

const api = vi.hoisted(() => ({ client: { POST: vi.fn(), DELETE: vi.fn(), PATCH: vi.fn() } }));
vi.mock("../lib/api", () => api);

function node(id: string, body: Partial<PatchNode> & Pick<PatchNode, "kind">): PatchNode {
  return { id, position: { x: 0, y: 0 }, ...body } as PatchNode;
}

const graph: PatchGraph = {
  nodes: [
    node("dev", { kind: "device", data: {} }),
    node("high", { kind: "channel", data: { channel_type: "nfm" } }),
  ],
  edges: [{ from: { node: "dev", port: "iq3" }, to: { node: "high", port: "iq" } }],
};

describe("dropNodes", () => {
  it("only edits the graph and forgets the nodes' state, leaving the engine to the server", () => {
    const finder = node("df", { kind: "df", data: {} });
    const snapshot = {
      version: 4,
      graph: {
        nodes: [...graph.nodes, finder],
        edges: [
          ...(graph.edges ?? []),
          { from: { node: "df", port: "iq" }, to: { node: "high", port: "iq" } },
        ],
      },
      rack: { slots: [{ node: "df", x: 0, y: 0, w: 1, h: 1 }] },
    } as unknown as WorkspaceSnapshot;
    let edited = snapshot;
    let edits = 0;
    const target = {
      edit: (change: (held: WorkspaceSnapshot) => WorkspaceSnapshot) => {
        edits += 1;
        edited = change(edited);
      },
    };
    useProcessorStore.getState().observe({
      type: "ProcessorUpdate",
      data: { node: "df", reading: { type: "df", reading: {} } },
    } as never);

    dropNodes(target, ["df", "dev"]);

    expect(edits).toBe(1);
    expect(edited.graph.nodes.map((kept) => kept.id)).toEqual(["high"]);
    expect(edited.graph.edges).toEqual([]);
    expect(edited.rack).toEqual({ slots: [] });
    expect(useProcessorStore.getState().byNode.df).toBeUndefined();
    expect(api.client.POST).not.toHaveBeenCalled();
    expect(api.client.DELETE).not.toHaveBeenCalled();
    expect(api.client.PATCH).not.toHaveBeenCalled();

    dropNodes(target, []);
    expect(edits).toBe(1);
  });
});
