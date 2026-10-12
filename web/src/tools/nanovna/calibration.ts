import type { NanoVnaCalibration, NanoVnaStandard } from "../../lib/types";

export interface CalibrationState {
  label: string;
  tone: string;
  ok: boolean;
  detail: string;
}

const TERM_FOR: Partial<Record<NanoVnaStandard, string>> = {
  open: "Er",
  short: "Es",
  thru: "Et",
};

const REFLECTION: readonly NanoVnaStandard[] = ["open", "short", "load"];

export function measuredStandards(calibration: NanoVnaCalibration): Set<NanoVnaStandard> {
  const measured = new Set(calibration.standards);
  for (const [standard, term] of Object.entries(TERM_FOR)) {
    if (calibration.error_terms.includes(term)) {
      measured.add(standard as NanoVnaStandard);
    }
  }
  return measured;
}

export function calibrationState(calibration: NanoVnaCalibration): CalibrationState {
  const measured = measuredStandards(calibration);
  const detail = calibration.raw === "" ? "Nothing measured" : calibration.raw;
  if (!calibration.applied) {
    return { label: "Uncalibrated", tone: "text-warn", ok: false, detail };
  }
  if (!REFLECTION.every((standard) => measured.has(standard))) {
    return { label: "Partial calibration", tone: "text-warn", ok: false, detail };
  }
  const through = measured.has("thru") ? ", with thru" : "";
  return { label: "Calibrated", tone: "text-ok", ok: true, detail: `${detail}${through}` };
}
