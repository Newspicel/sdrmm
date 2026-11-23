import { describe, expect, it } from "vitest";
import { isViewport } from "./Canvas";

describe("isViewport", () => {
  it("restores only a finite pan inside the zoom limits", () => {
    expect(isViewport({ x: 10, y: -4, zoom: 1 })).toBe(true);
    expect(isViewport({ x: 10, y: -4, zoom: 9 })).toBe(false);
    expect(isViewport({ x: Number.NaN, y: 0, zoom: 1 })).toBe(false);
    expect(isViewport("x")).toBe(false);
  });
});
