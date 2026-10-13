import { describe, expect, it } from "vitest";
import { climbRate, metres, wind } from "./weatherFormat";

describe("weatherFormat", () => {
  it("signs climb rates", () => {
    expect(climbRate(5)).toBe("+5.0 m/s");
    expect(climbRate(-12.34)).toBe("-12.3 m/s");
    expect(climbRate(null)).toBeUndefined();
  });

  it("groups metres", () => {
    expect(metres(31_250.6)).toBe("31,251 m");
  });

  it("names wind with or without a direction", () => {
    expect(wind({ wind_speed_ms: 2, wind_dir_deg: 270 })).toBe("2.0 m/s from 270°");
    expect(wind({ wind_speed_ms: 2 })).toBe("2.0 m/s");
    expect(wind({ wind_dir_deg: 270 })).toBeUndefined();
  });
});
