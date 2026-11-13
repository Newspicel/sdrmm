import type { Options } from "../../../components/controls";
import { bearingHue } from "../../../gl/spatial";
import type { SpatialSpectrumFrame } from "../../../lib/frame";
import type { ViewMark } from "../../../lib/types";
import type { BearingFrame } from "./bearing";

export { bearingHue, columnArgmax } from "../../../gl/spatial";

export type SpatialView = "map" | "trail";

export const BEARING_TICKS = [0, 90, 180, 270, 360] as const;

export const VIEW_OPTIONS: Options<SpatialView> = [
  { value: "map", label: "Map", title: "Bearing over frequency" },
  { value: "trail", label: "Trail", title: "Frequency over time, colour is bearing" },
];

export function binHz(col: number, bins: number, centerHz: number, spanHz: number): number {
  return centerHz - spanHz / 2 + ((col + 0.5) * spanHz) / bins;
}

export function rowDeg(row: number, bearings: number): number {
  return bearings > 0 ? (row * 360) / bearings : 0;
}

export function rowShift(offsetDeg: number, bearings: number): number {
  if (bearings <= 0) {
    return 0;
  }
  return ((Math.round((offsetDeg * bearings) / 360) % bearings) + bearings) % bearings;
}

export function rotateRows(
  frame: SpatialSpectrumFrame,
  offsetDeg: number,
  out?: Uint8Array,
): Uint8Array {
  const { bearings, bins, cells } = frame;
  const length = bearings * bins;
  const rotated = out !== undefined && out.length === length ? out : new Uint8Array(length);
  const shift = rowShift(offsetDeg, bearings);
  for (let row = 0; row < bearings; row++) {
    const source = (row - shift + bearings) % bearings;
    rotated.set(cells.subarray(source * bins, source * bins + bins), row * bins);
  }
  return rotated;
}

export function hoverAt(
  x: number,
  y: number,
  box: { w: number; h: number },
  frame: SpatialSpectrumFrame,
): { hz: number; deg: number; col: number; row: number } {
  const col = Math.min(
    frame.bins - 1,
    Math.max(0, Math.floor((x / Math.max(1, box.w)) * frame.bins)),
  );
  const row = Math.min(
    frame.bearings - 1,
    Math.max(0, Math.floor((y / Math.max(1, box.h)) * frame.bearings)),
  );
  return {
    hz: binHz(col, frame.bins, frame.centerHz, frame.spanHz),
    deg: rowDeg(row, frame.bearings),
    col,
    row,
  };
}

export function belowPeakDb(level: number, frame: SpatialSpectrumFrame): number {
  return (level / 255 - 1) * (frame.dbMax - frame.dbMin);
}

function mhz(hz: number): string {
  return `${(hz / 1e6).toFixed(3)} MHz`;
}

function degreeText(value: number): string {
  return `${Math.round(bearingHue(value)) % 360}°`;
}

export function cursorText(hz: number, bearing: number, db: number | null): string {
  const parts = [mhz(hz), degreeText(bearing)];
  if (db !== null) {
    parts.push(`${db.toFixed(0)} dB`);
  }
  return parts.join(" ");
}

/** A mark's bearing in `frame`; a true bearing the reading lacks comes from the heading. */
export function markBearing(mark: ViewMark, frame: BearingFrame, offsetDeg = 0): number {
  return frame === "true" ? (mark.true_y ?? mark.y + offsetDeg) : mark.y;
}
