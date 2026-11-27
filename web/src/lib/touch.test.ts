import { describe, expect, it } from "vitest";
import { touchesWith } from "./touch";

describe("touchesWith", () => {
  it("turns touch on at a finger", () => {
    expect(touchesWith("touch", false)).toBe(true);
  });

  it("turns it off at the mouse or trackpad", () => {
    expect(touchesWith("mouse", true)).toBe(false);
  });

  it("leaves it as it was for a pen", () => {
    expect(touchesWith("pen", true)).toBe(true);
    expect(touchesWith("pen", false)).toBe(false);
  });
});
