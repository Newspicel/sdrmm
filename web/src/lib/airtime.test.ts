import { describe, expect, it } from "vitest";
import { percentOf } from "./airtime";

describe("airtime", () => {
  it("keeps a decimal below ten percent", () => {
    expect(percentOf(0.004)).toBe("0.4%");
    expect(percentOf(0.314)).toBe("31%");
  });
});
