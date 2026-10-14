import { describe, expect, it } from "vitest";
import { anchorsOf, sameAnchors, visualCenter } from "./portAnchor";

describe("visualCenter", () => {
  it("reads a row's place on screen back into the node's own pixels", () => {
    expect(visualCenter({ top: 150, height: 10 }, { top: 50, height: 200 }, 0.5, 1)).toBe(209);
  });
});

describe("anchorsOf", () => {
  it("places every tracked port on its row", () => {
    const marks = new Map([
      ["iq", 110],
      ["iq2", 140],
    ]);
    expect(anchorsOf(marks, (top) => top)).toEqual({ iq: 110, iq2: 140 });
  });
});

describe("sameAnchors", () => {
  it("tells a moved or added port apart from a stable set", () => {
    expect(sameAnchors({ iq: 110 }, { iq: 110 })).toBe(true);
    expect(sameAnchors({ iq: 110 }, { iq: 120 })).toBe(false);
    expect(sameAnchors({ iq: 110 }, { iq: 110, iq2: 140 })).toBe(false);
  });
});
