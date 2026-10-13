import { describe, expect, it } from "vitest";
import type { LaneWire, NodeBodyOf, PatchGraph } from "../lib/types";
import { catalogBody } from "../test/catalog";
import { nodeSize } from "./graph";
import { arrayPlacement, makeArray } from "./makeArray";

const BODY = catalogBody("array") as NodeBodyOf<"array">;

function radio(): PatchGraph {
  return {
    nodes: [{ id: "kraken", position: { x: 100, y: 40 }, kind: "device", data: {} }],
    edges: [],
  };
}

const WIRES: LaneWire[] = [
  { from: "iq", to: "lane" },
  { from: "iq2", to: "lane2" },
];

describe("makeArray", () => {
  it("adds the array and exactly the wires the server offered", () => {
    const graph = makeArray(radio(), "kraken", WIRES, BODY, "array:1");
    expect(graph.nodes.map((node) => node.kind)).toEqual(["device", "array"]);
    expect(graph.edges).toEqual(
      WIRES.map((wire) => ({
        from: { node: "kraken", port: wire.from },
        to: { node: "array:1", port: wire.to },
      })),
    );
    const array = graph.nodes[1];
    expect(array?.kind === "array" && array.data).toEqual(BODY.data);
  });

  it("places the array right of the radio", () => {
    expect(arrayPlacement(radio(), "kraken")).toEqual({
      x: 100 + nodeSize("device").w + 80,
      y: 40,
    });
  });
});
