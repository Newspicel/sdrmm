import { describe, expect, it } from "vitest";
import type { PatchGraph, PatchNode } from "../lib/types";
import { movedGeometry, withGeometry } from "./geometry";
import { naturalSize, nodeSize } from "./graph";

function node(id: string, x: number): PatchNode {
  return { id, kind: "scope", position: { x, y: 0 } };
}

const graph: PatchGraph = { nodes: [node("a", 0), node("b", 0)], edges: [] };
const natural = nodeSize("scope");

describe("geometry", () => {
  it("names only the nodes that moved or resized", () => {
    const moved = movedGeometry(
      [
        { id: "a", position: { x: 40, y: 0 }, width: natural.w, height: natural.h },
        { id: "b", position: { x: 0, y: 0 }, width: natural.w, height: natural.h },
      ],
      graph,
      1,
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
      1,
    );
    const written = withGeometry(others, moved);
    expect(written.nodes[0]).toMatchObject({ position: { x: 40, y: 0 }, size: { w: 500, h: 300 } });
    expect(written.nodes[1]?.position.x).toBe(99);
  });

  it("never saves a larger font size as a resize", () => {
    const grown = naturalSize("scope", 1.2);
    const moved = movedGeometry(
      [{ id: "a", position: { x: 0, y: 0 }, width: grown.w, height: grown.h }],
      graph,
      1.2,
    );
    expect(moved.size).toBe(0);
  });

  it("saves a size the operator dragged at a larger font size as it is", () => {
    const moved = movedGeometry(
      [{ id: "a", position: { x: 0, y: 0 }, width: 640, height: 360 }],
      graph,
      1.2,
    );
    expect(moved.get("a")?.size).toEqual({ w: 640, h: 360 });
  });

  it("grows a face it draws with the font size, rounded to whole pixels", () => {
    const grown = naturalSize("scope", 1.15);
    expect(grown.w).toBe(Math.round((natural.w * 115) / 100));
    expect(grown.h).toBe(Math.round((natural.h ?? 0) * 1.15));
    expect(naturalSize("scope", 1)).toEqual(natural);
  });
});
