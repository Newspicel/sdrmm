import { describe, expect, it } from "vitest";
import { formatNumber, formatValue } from "./values";

describe("field values", () => {
  it("formats numbers by unit", () => {
    expect(formatNumber(35000, "feet", 0)).toBe("35000 ft");
    expect(formatNumber(271.4, "degrees", 0)).toBe("271°");
    expect(formatNumber(12.345, "none", 1)).toBe("12.3");
    expect(formatNumber(145_800_000, "hz", 0)).toBe("145.8 MHz");
  });

  it("formats every value type", () => {
    expect(formatValue({ type: "text", value: "DLH123" })).toBe("DLH123");
    expect(formatValue({ type: "flag", value: true })).toBe("Yes");
    expect(formatValue({ type: "time", value: "2026-09-21T10:00:00Z" })).toBe(
      "2026-09-21 10:00:00",
    );
    expect(formatValue({ type: "position", value: { lat: 52.5, lon: 13.4 } })).toBe(
      "52.50000, 13.40000",
    );
  });
});
