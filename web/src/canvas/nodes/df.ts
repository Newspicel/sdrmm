import type { Options } from "../../components/controls";
import {
  RELATIVE_MARKS,
  type RoseMark,
  type RoseNeedle,
  type RoseWedge,
  TRUE_MARKS,
} from "../../components/Rose";
import { GATE_TEXT } from "../../lib/arrays";
import type { DfPeak, DfReading, ProcessorGate } from "../../lib/types";
import type { Chip } from "./ProcessorHealth";

export const NEEDS_HEADING = "Needs heading";

export type BearingFrame = "relative" | "true";

export function frameOptions(trueAvailable: boolean): Options<BearingFrame> {
  return [
    { value: "relative", label: "Rel", title: "Relative to the array" },
    {
      value: "true",
      label: "True",
      title: trueAvailable ? "From true north" : NEEDS_HEADING,
      disabled: !trueAvailable,
    },
  ];
}

export function shownFrame(picked: BearingFrame | null, trueAvailable: boolean): BearingFrame {
  if (!trueAvailable) {
    return "relative";
  }
  return picked ?? "true";
}

export function roseLetters(trueFrame: boolean): readonly RoseMark[] {
  return trueFrame ? TRUE_MARKS : RELATIVE_MARKS;
}

export function frameRotation(reading: DfReading | null, frame: BearingFrame): number {
  return frame === "true" ? (reading?.azimuth_deg ?? 0) : 0;
}

export function peakDeg(peak: DfPeak, frame: BearingFrame): number | null {
  return frame === "true" ? (peak.true_deg ?? null) : peak.relative_deg;
}

function mirrorDeg(peak: DfPeak, frame: BearingFrame): number | null {
  return frame === "true" ? (peak.mirror_true_deg ?? null) : (peak.mirror_deg ?? null);
}

export function roseNeedles(peaks: readonly DfPeak[], frame: BearingFrame): RoseNeedle[] {
  const needles: RoseNeedle[] = [];
  peaks.forEach((peak, rank) => {
    const deg = peakDeg(peak, frame);
    if (deg !== null) {
      needles.push({ deg, weight: rank === 0 ? "primary" : "secondary" });
    }
    const mirror = mirrorDeg(peak, frame);
    if (mirror !== null) {
      needles.push({ deg: mirror, weight: "mirror" });
    }
  });
  return needles;
}

export function roseWedge(peaks: readonly DfPeak[], frame: BearingFrame): RoseWedge | null {
  const first = peaks[0];
  const deg = first === undefined ? null : peakDeg(first, frame);
  return first === undefined || deg === null ? null : { deg, sigmaDeg: first.sigma_deg };
}

function wrapped(deg: number): number {
  return (((Math.round(deg) % 360) + 360) % 360) % 360;
}

export function bearingLabel(deg: number): string {
  return `${String(wrapped(deg)).padStart(3, "0")}°`;
}

export function peakText(peak: DfPeak, frame: BearingFrame): string {
  const deg = peakDeg(peak, frame);
  return deg === null ? "-" : bearingLabel(deg);
}

const READING_CHIPS: readonly {
  on: (reading: DfReading) => boolean;
  label: string;
  title: string;
}[] = [
  { on: (reading) => reading.squelched, label: "Squelch", title: "Peak below squelch" },
  {
    on: (reading) => reading.azimuth_deg == null,
    label: "No heading",
    title: "Wire a GPS with heading",
  },
  {
    on: (reading) => reading.station == null,
    label: "No position",
    title: "Wire a GPS to the array to send bearings",
  },
  {
    on: (reading) => reading.aliasing,
    label: "Aliased",
    title: "Elements more than half a wavelength apart",
  },
  {
    on: (reading) => reading.mode_aliasing === true,
    label: "Mode alias",
    title: "Too high for phase modes on this circle",
  },
  {
    on: (reading) => reading.mirror === true,
    label: "Mirror",
    title: "A line array cannot tell front from back",
  },
  {
    on: (reading) => reading.rotating === true,
    label: "Rotating",
    title: "Turning too fast, blocks skipped",
  },
  {
    on: (reading) => reading.singular === true,
    label: "Singular",
    title: "Covariance could not be inverted",
  },
  {
    on: (reading) => reading.table_out_of_range === true,
    label: "Table off",
    title: "Outside the calibration table",
  },
];

export const STALE_CHIP: Chip = {
  label: "Stale",
  title: "No reading for three report periods",
  danger: true,
};

export function dfChips(
  reading: DfReading | null,
  gate: ProcessorGate | null,
  stale: boolean,
): Chip[] {
  const chips: Chip[] = [];
  if (gate !== null) {
    chips.push({ label: GATE_TEXT[gate], title: "The array holds this finder", danger: true });
  }
  if (stale) {
    chips.push(STALE_CHIP);
  }
  if (reading !== null) {
    for (const chip of READING_CHIPS) {
      if (chip.on(reading)) {
        chips.push({ label: chip.label, title: chip.title, danger: false });
      }
    }
  }
  return chips;
}
