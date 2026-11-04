import type { BeamformerReading, LaneWeight } from "../../lib/types";

export const NULL_CLOSE_DEG = 10;
export const PATTERN_FLOOR_DB = 40;

export function amplitudePercent(weights: readonly LaneWeight[]): number[] {
  const linear = weights.map((weight) => 10 ** (weight.amplitude_db / 20));
  const top = Math.max(0, ...linear);
  return linear.map((value) => (top > 0 ? Math.round((value / top) * 100) : 0));
}

export function weightTitle(lane: number, weight: LaneWeight): string {
  return `L${lane + 1} ${weight.amplitude_db.toFixed(1)} dB ${Math.round(weight.phase_deg)}°`;
}

function wrapDeg(deg: number): number {
  const rounded = Math.round(deg * 10) / 10;
  return ((rounded % 360) + 360) % 360;
}

export function angleGap(a: number, b: number): number {
  const gap = Math.abs(wrapDeg(a) - wrapDeg(b));
  return Math.min(gap, 360 - gap);
}

export function beamChips(reading: BeamformerReading | null): { label: string; title: string }[] {
  if (reading === null) {
    return [];
  }
  const chips: { label: string; title: string }[] = [];
  if (reading.no_steer === true) {
    chips.push({ label: "No steer", title: "No bearing to steer at" });
  }
  if (reading.steer_stale === true) {
    chips.push({ label: "Stale", title: "Last bearing is old" });
  }
  if (reading.singular === true) {
    chips.push({ label: "Singular", title: "Lanes too alike to solve" });
  }
  if ((reading.resets ?? 0) > 0 || reading.diverged === true) {
    chips.push({ label: "Reset", title: `Weights restarted ${reading.resets ?? 0} times` });
  }
  if (reading.band_full === true) {
    chips.push({ label: "Band full", title: "Band wider than the lanes" });
  }
  const steer = reading.steer_deg;
  if (steer != null && reading.nulls_deg.some((deg) => angleGap(deg, steer) < NULL_CLOSE_DEG)) {
    chips.push({ label: "Null close", title: "A null sits next to the steer" });
  }
  return chips;
}

export function patternPath(pattern: readonly number[], radius: number, centre: number): string {
  if (pattern.length === 0) {
    return "";
  }
  const points = pattern.map((level, index) => {
    const deg = (index * 360) / pattern.length;
    const reach = (Math.min(255, Math.max(0, level)) / 255) * radius;
    const radians = (deg * Math.PI) / 180;
    return `${(centre + reach * Math.sin(radians)).toFixed(1)} ${(centre - reach * Math.cos(radians)).toFixed(1)}`;
  });
  return `M${points.join("L")}Z`;
}

export function polarTick(deg: number, inner: number, outer: number, centre: number): string {
  const radians = (deg * Math.PI) / 180;
  const x = (r: number) => (centre + r * Math.sin(radians)).toFixed(1);
  const y = (r: number) => (centre - r * Math.cos(radians)).toFixed(1);
  return `M${x(inner)} ${y(inner)}L${x(outer)} ${y(outer)}`;
}
