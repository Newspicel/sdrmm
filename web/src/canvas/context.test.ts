import { describe, expect, it } from "vitest";
import { samePick } from "./context";

describe("samePick", () => {
  it("compares plain picks by value", () => {
    expect(samePick({ a: [1, { b: "x" }] }, { a: [1, { b: "x" }] })).toBe(true);
    expect(samePick({ a: [1, 2] }, { a: [1, 3] })).toBe(false);
    expect(samePick({ a: 1 }, { a: 1, b: undefined })).toBe(false);
    expect(samePick(null, undefined)).toBe(false);
    expect(samePick(Number.NaN, Number.NaN)).toBe(true);
  });

  it("compares a Map or a function by identity", () => {
    const map = new Map([["a", 1]]);
    expect(samePick(map, map)).toBe(true);
    expect(samePick(map, new Map([["a", 1]]))).toBe(false);
    expect(samePick({ f: () => 1 }, { f: () => 1 })).toBe(false);
  });
});
