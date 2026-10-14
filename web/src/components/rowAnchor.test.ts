import { describe, expect, it } from "vitest";
import { anchorShift } from "./rowAnchor";

describe("row anchor", () => {
  it("follows the opened row when rows land above it", () => {
    expect(anchorShift({ key: "a", top: 40 }, { key: "a", top: 100 })).toBe(60);
  });

  it("does not scroll when another row opens", () => {
    expect(anchorShift({ key: "a", top: 40 }, { key: "b", top: 100 })).toBe(0);
  });

  it("does not scroll when nothing is open", () => {
    expect(anchorShift(null, { key: "a", top: 100 })).toBe(0);
    expect(anchorShift({ key: "a", top: 40 }, null)).toBe(0);
  });
});
