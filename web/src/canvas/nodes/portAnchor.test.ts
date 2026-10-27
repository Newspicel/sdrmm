import { describe, expect, it } from "vitest";
import { anchorsOf, sameAnchors, visualCenter } from "./portAnchor";
import { faceAnchors } from "./useFaceLayout";

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

function box(top: number, height: number): DOMRect {
  return { top, height } as DOMRect;
}

describe("faceAnchors", () => {
  it("reads the host rect once however many ports a face tracks", () => {
    const reads = { host: 0, marks: 0 };
    const host = {
      offsetHeight: 200,
      clientTop: 1,
      getBoundingClientRect: () => {
        reads.host += 1;
        return box(100, 200);
      },
    };
    const mark = (top: number) =>
      ({
        getBoundingClientRect: () => {
          reads.marks += 1;
          return box(top, 10);
        },
      }) as HTMLElement;
    const marks = new Map([
      ["iq", mark(110)],
      ["audio", mark(130)],
      ["events", mark(150)],
    ]);

    expect(faceAnchors(host, marks)).toEqual({ iq: 14, audio: 34, events: 54 });
    expect(reads).toEqual({ host: 1, marks: 3 });
  });
});
