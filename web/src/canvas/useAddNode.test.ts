import { describe, expect, it, vi } from "vitest";
import type { Workspace } from "./context";

const place = vi.hoisted(() => vi.fn(() => ({ x: 120, y: 240 })));
const edits = vi.hoisted(() => [] as ((snapshot: unknown) => unknown)[]);
const workspace = vi.hoisted(
  () =>
    ({
      graph: { nodes: [], edges: [] },
      edit: vi.fn((edit: (snapshot: unknown) => unknown) => edits.push(edit)),
      select: vi.fn(),
      apply: vi.fn(),
    }) as unknown as Workspace,
);

vi.mock("./context", () => ({ useWorkspaceContext: () => workspace }));
vi.mock("./placement", () => ({ useNodePlacement: () => place }));

const { CATALOG } = await import("../test/catalog");
Object.assign(workspace, { context: { catalog: CATALOG, channelTypes: [], bound: new Map() } });
const { useAddNode } = await import("./useAddNode");

describe("useAddNode", () => {
  it("places a node once, so replaying the edit cannot move it or the viewport", () => {
    edits.length = 0;
    place.mockClear();
    useAddNode()("scope");

    expect(place).toHaveBeenCalledTimes(1);
    const edit = edits[0];
    expect(edit).toBeDefined();
    const first = edit?.({ graph: { nodes: [], edges: [] } }) as {
      graph: { nodes: { position: unknown }[] };
    };
    const second = edit?.({ graph: { nodes: [], edges: [] } }) as {
      graph: { nodes: { position: unknown }[] };
    };
    expect(place).toHaveBeenCalledTimes(1);
    expect(first.graph.nodes[0]?.position).toEqual({ x: 120, y: 240 });
    expect(second.graph.nodes[0]?.position).toEqual({ x: 120, y: 240 });
  });

  it("takes a dropped position without consulting the viewport", () => {
    edits.length = 0;
    place.mockClear();
    useAddNode()("scope", undefined, { x: 5, y: 6 });

    expect(place).not.toHaveBeenCalled();
    const placed = edits[0]?.({ graph: { nodes: [], edges: [] } }) as {
      graph: { nodes: { position: unknown }[] };
    };
    expect(placed.graph.nodes[0]?.position).toEqual({ x: 5, y: 6 });
  });
});
