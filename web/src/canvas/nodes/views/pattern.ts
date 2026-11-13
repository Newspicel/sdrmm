import type { LaneWeight } from "../../../lib/types";

export const NULL_CLOSE_DEG = 10;

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
