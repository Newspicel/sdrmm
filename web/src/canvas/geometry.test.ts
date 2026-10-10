import { describe, expect, it } from "vitest";
import type { PatchGraph, PatchNode } from "../lib/types";
import { movedGeometry, withGeometry } from "./geometry";
import { NODE_SIZE } from "./graph";

function node(id: string, x: number): PatchNode {
  return { id, kind: "scope", position: { x, y: 0 } };
}

const graph: PatchGraph = { nodes: [node("a", 0), node("b", 0)], edges: [] };
const natural = NODE_SIZE.scope;

describe("geometry", () => {
  it("names only the nodes that moved or resized", () => {
    const moved = movedGeometry(
      [
        { id: "a", position: { x: 40, y: 0 }, width: natural.w, height: natural.h },
        { id: "b", position: { x: 0, y: 0 }, width: natural.w, height: natural.h },
      ],
      graph,
    );
    expect([...moved.keys()]).toEqual(["a"]);
  });

  it("writes them without touching the rest", () => {
    const others: PatchGraph = {
      nodes: [node("a", 0), node("b", 99)],
      edges: [],
    };
    const moved = movedGeometry(
      [{ id: "a", position: { x: 40, y: 0 }, width: 500, height: 300 }],
      graph,
    );
    const written = withGeometry(others, moved);
    expect(written.nodes[0]).toMatchObject({ position: { x: 40, y: 0 }, size: { w: 500, h: 300 } });
    expect(written.nodes[1]?.position.x).toBe(99);
  });
});
