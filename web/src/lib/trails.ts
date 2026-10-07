import { loraPosition } from "./lora";
import type { DecoderEvent, DecoderKind } from "./types";

export const TRAIL_CAPACITY = 512;
export const TRAIL_TRIM = TRAIL_CAPACITY / 4;
export const TRAIL_TOLERANCE_M = 25;

const EARTH_M = 6_371_000;
const RAD = Math.PI / 180;

export type LonLat = [number, number];

export interface Trail {
  points: LonLat[];
  lastAt: number;
  dirX: number;
  dirY: number;
  reach: number;
  line: LonLat[] | null;
}

const index = new Map<DecoderKind, Map<string, Trail>>();
const NO_TRAILS: ReadonlyMap<string, Trail> = new Map();

export function geoPosition(
  lat: number | null | undefined,
  lon: number | null | undefined,
): [number, number] | null {
  if (lat == null || lon == null || !Number.isFinite(lat) || !Number.isFinite(lon)) {
    return null;
  }
  if (Math.abs(lat) > 90 || Math.abs(lon) > 180) {
    return null;
  }
  return [lon, lat];
}

export function emptyTrail(): Trail {
  return { points: [], lastAt: -Infinity, dirX: 0, dirY: 0, reach: 0, line: null };
}

export function trailPosition(event: DecoderEvent): LonLat | null {
  switch (event.kind) {
    case "adsb":
    case "ais":
    case "aprs":
    case "radiosonde":
      return geoPosition(event.data.lat, event.data.lon);
    case "lora": {
      const position = loraPosition(event.data);
      return geoPosition(position?.lat, position?.lon);
    }
    default:
      return null;
  }
}

export function recordTrail(kind: DecoderKind, id: string, event: DecoderEvent, at: number): void {
  const point = trailPosition(event);
  if (point === null) {
    return;
  }
  let trails = index.get(kind);
  if (trails === undefined) {
    trails = new Map();
    index.set(kind, trails);
  }
  let trail = trails.get(id);
  if (trail === undefined) {
    trail = emptyTrail();
    trails.set(id, trail);
  }
  extendTrail(trail, point, at);
}

export function trailsOf(kind: DecoderKind): ReadonlyMap<string, Trail> {
  return index.get(kind) ?? NO_TRAILS;
}

export function dropTrail(kind: DecoderKind, id: string): void {
  index.get(kind)?.delete(id);
}

export function clearTrails(): void {
  index.clear();
}

export function trailLine(trail: Trail): LonLat[] {
  trail.line ??= trail.points.slice();
  return trail.line;
}

export function extendTrail(trail: Trail, point: LonLat, at: number): boolean {
  if (at < trail.lastAt) {
    return false;
  }
  trail.lastAt = at;
  const { points } = trail;
  const last = points.at(-1);
  if (last === undefined) {
    points.push(point);
    trail.line = null;
    return true;
  }
  const anchor = points.at(-2);
  if (anchor !== undefined && extendsSegment(trail, anchor, point)) {
    points[points.length - 1] = point;
    trail.line = null;
    return true;
  }
  const [dx, dy] = offsetM(last, point);
  const step = Math.hypot(dx, dy);
  if (step < TRAIL_TOLERANCE_M) {
    return false;
  }
  points.push(point);
  trail.dirX = dx / step;
  trail.dirY = dy / step;
  trail.reach = step;
  trail.line = null;
  if (points.length > TRAIL_CAPACITY) {
    points.splice(0, TRAIL_TRIM);
  }
  return true;
}

function extendsSegment(trail: Trail, anchor: LonLat, point: LonLat): boolean {
  const [dx, dy] = offsetM(anchor, point);
  const along = dx * trail.dirX + dy * trail.dirY;
  const across = Math.abs(dx * trail.dirY - dy * trail.dirX);
  if (across > TRAIL_TOLERANCE_M || along < trail.reach) {
    return false;
  }
  trail.reach = along;
  return true;
}

export function offsetM(from: LonLat, to: LonLat): [number, number] {
  const dLon = ((((to[0] - from[0] + 180) % 360) + 360) % 360) - 180;
  const x = dLon * RAD * Math.cos(from[1] * RAD) * EARTH_M;
  const y = (to[1] - from[1]) * RAD * EARTH_M;
  return [x, y];
}
