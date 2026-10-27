import { describe, expect, it } from "vitest";
import { toneLabel } from "./tone";

describe("toneLabel", () => {
  it("names what is under the carrier the way a radio does", () => {
    expect(toneLabel({})).toBe("");
    expect(toneLabel({ ctcss_hz: 88.5 })).toBe("CTCSS 88.5 Hz");
    expect(toneLabel({ ctcss_hz: 100 })).toBe("CTCSS 100.0 Hz");
    expect(toneLabel({ dcs_code: 23 })).toBe("DCS 023");
    expect(toneLabel({ dcs_code: 754 })).toBe("DCS 754");
    expect(toneLabel({ ctcss_hz: 88.5, dcs_code: 23 })).toBe("CTCSS 88.5 Hz · DCS 023");
  });
});
