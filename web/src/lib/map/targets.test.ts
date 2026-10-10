import { describe, expect, it } from "vitest";
import { PRESET_COLORS } from "./basemap";
import { labelInk } from "./targets";

describe("labelInk", () => {
  it("writes dark on a white halo over a light map", () => {
    expect(labelInk(PRESET_COLORS.liberty.land)).toEqual({ text: "#15171a", halo: "#ffffff" });
    expect(labelInk(PRESET_COLORS.positron.land).halo).toBe("#ffffff");
  });

  it("writes light on a dark halo over a dark map", () => {
    expect(labelInk(PRESET_COLORS.dark.land)).toEqual({ text: "#f4f6f8", halo: "#0c0d0f" });
    expect(labelInk(PRESET_COLORS.fiord.land).halo).toBe("#0c0d0f");
  });

  it("falls back to dark ink for a colour it cannot read", () => {
    expect(labelInk("rgb(1, 2, 3)").text).toBe("#15171a");
  });
});
