import { describe, expect, it } from "vitest";
import type { NanoVnaCalibration, NanoVnaStandard } from "../../lib/types";
import { calibrationState, measuredStandards } from "./calibration";

function status(
  standards: NanoVnaStandard[],
  errorTerms: string[],
  applied: boolean,
): NanoVnaCalibration {
  return { port: "port", standards, error_terms: errorTerms, applied, raw: "raw" };
}

describe("calibration status", () => {
  it("reads a finished two-port calibration from its error terms", () => {
    const full = status(["load", "isolation"], ["Es", "Er", "Et"], true);
    expect([...measuredStandards(full)].toSorted()).toEqual([
      "isolation",
      "load",
      "open",
      "short",
      "thru",
    ]);
    expect(calibrationState(full)).toMatchObject({ label: "Calibrated", ok: true });
  });

  it("calls a finished one-port calibration complete", () => {
    expect(calibrationState(status(["load"], ["Es", "Er"], true)).label).toBe("Calibrated");
  });

  it("counts standards measured but not yet applied", () => {
    const pending = status(["load", "open", "short"], [], false);
    expect(measuredStandards(pending).has("short")).toBe(true);
    expect(calibrationState(pending).label).toBe("Uncalibrated");
  });

  it("warns when correction runs without open and short", () => {
    expect(calibrationState(status(["load"], ["Er"], true)).label).toBe("Partial calibration");
  });
});
