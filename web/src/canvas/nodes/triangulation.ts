import type { FusionGridFrame } from "../../lib/frame";
import { feedSources } from "../../lib/resolved";
import type { DfStation, ResolvedPatch } from "../../lib/types";

export const NO_SOURCES = "Wire direction finders in";

export interface Box {
  w: number;
  h: number;
}

export type GridBounds = Pick<FusionGridFrame, "south" | "west" | "north" | "east">;

export function geoToGrid(
  point: { lat: number; lon: number },
  frame: GridBounds,
  box: Box,
): { x: number; y: number } {
  const width = frame.east - frame.west;
  const height = frame.north - frame.south;
  return {
    x: width === 0 ? box.w / 2 : ((point.lon - frame.west) / width) * box.w,
    y: height === 0 ? box.h / 2 : ((frame.north - point.lat) / height) * box.h,
  };
}

export function insideBox(point: { x: number; y: number }, box: Box): boolean {
  return point.x >= 0 && point.x <= box.w && point.y >= 0 && point.y <= box.h;
}

export function fusionSources(resolved: ResolvedPatch | null, node: string): number {
  return feedSources(resolved, node, "events").length;
}

export function stationAge(station: DfStation, now: number): string {
  const seen = Date.parse(station.last_seen);
  if (Number.isNaN(seen)) {
    return "just now";
  }
  const seconds = Math.max(0, Math.round((now - seen) / 1_000));
  if (seconds < 5) {
    return "just now";
  }
  if (seconds < 90) {
    return `${seconds}s ago`;
  }
  return `${Math.round(seconds / 60)}m ago`;
}

export function stationBearing(station: DfStation): string {
  const deg = station.last_bearing_deg;
  return deg === undefined || !Number.isFinite(deg) ? "-" : `${deg.toFixed(1)}°`;
}

export function stationAlign(station: DfStation): string {
  const deg = station.align_deg;
  return deg === undefined || deg === null || !Number.isFinite(deg)
    ? "-"
    : `${deg > 0 ? "+" : ""}${deg.toFixed(1)}°`;
}

export function stationSigma(station: DfStation): string {
  const sigma = station.sigma_deg;
  return sigma === undefined || !Number.isFinite(sigma) || sigma <= 0
    ? "-"
    : `${sigma.toFixed(1)}°`;
}
