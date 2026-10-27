import { describe, expect, it } from "vitest";
import { deepEqual } from "./deepEqual";

describe("deepEqual", () => {
  it("reads a reordered object as unchanged", () => {
    expect(deepEqual({ a: 1, b: { c: 2 } }, { b: { c: 2 }, a: 1 })).toBe(true);
  });

  it("holds array order", () => {
    expect(deepEqual([1, 2], [2, 1])).toBe(false);
    expect(deepEqual([1, 2], [1, 2])).toBe(true);
    expect(deepEqual([1, 2], [1, 2, 3])).toBe(false);
  });

  it("ignores a field left undefined", () => {
    expect(deepEqual({ a: 1, b: undefined }, { a: 1 })).toBe(true);
    expect(deepEqual({ a: 1, b: null }, { a: 1 })).toBe(false);
  });

  it("separates an array from an object", () => {
    expect(deepEqual([], {})).toBe(false);
    expect(deepEqual(null, {})).toBe(false);
    expect(deepEqual(1, "1")).toBe(false);
  });
});
