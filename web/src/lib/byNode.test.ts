import { describe, expect, it } from "vitest";
import { omitNodes } from "./byNode";

describe("omitNodes", () => {
  it("returns the same record when nothing listed is held", () => {
    const record = { a: 1 };
    expect(omitNodes(record, ["b"])).toBe(record);
    expect(omitNodes(record, ["a"])).toEqual({});
  });
});
