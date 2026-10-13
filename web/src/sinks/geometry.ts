import { unwrapTrail } from "../lib/map/bounds";
import type { LatLon } from "../lib/types";

const EARTH_RADIUS_M = 6_371_000;
export const RING_POINTS = 96;

export type Lnglat = [number, number];

export function destination(from: LatLon, degrees: number, distanceM: number): Lnglat {
  const bearing = (degrees * Math.PI) / 180;
  const angular = distanceM / EARTH_RADIUS_M;
  const phi = (from.lat * Math.PI) / 180;
  const lambda = (from.lon * Math.PI) / 180;
  const sinPhi =
    Math.sin(phi) * Math.cos(angular) + Math.cos(phi) * Math.sin(angular) * Math.cos(bearing);
  const phi2 = Math.asin(Math.min(1, Math.max(-1, sinPhi)));
  const lambda2 =
    lambda +
    Math.atan2(
      Math.sin(bearing) * Math.sin(angular) * Math.cos(phi),
      Math.cos(angular) - Math.sin(phi) * sinPhi,
    );
  return [(((lambda2 * 180) / Math.PI + 540) % 360) - 180, (phi2 * 180) / Math.PI];
}

export function lnglat(at: LatLon): Lnglat {
  return [at.lon, at.lat];
}

export function ellipseRing(
  center: LatLon,
  semiMajorM: number,
  semiMinorM: number,
  bearingDeg: number,
): Lnglat[] {
  const radians = (bearingDeg * Math.PI) / 180;
  const ring: Lnglat[] = [];
  for (let step = 0; step <= RING_POINTS; step++) {
    const angle = (step / RING_POINTS) * Math.PI * 2;
    const along = semiMajorM * Math.cos(angle);
    const across = semiMinorM * Math.sin(angle);
    const east = along * Math.sin(radians) + across * Math.cos(radians);
    const north = along * Math.cos(radians) - across * Math.sin(radians);
    ring.push(
      destination(center, (Math.atan2(east, north) * 180) / Math.PI, Math.hypot(east, north)),
    );
  }
  return unwrapTrail(ring);
}

export function circleRing(center: LatLon, radiusM: number): Lnglat[] {
  const ring: Lnglat[] = [];
  for (let step = 0; step <= RING_POINTS; step += 1) {
    ring.push(destination(center, (360 * step) / RING_POINTS, radiusM));
  }
  const unwrapped = unwrapTrail(ring);
  const first = unwrapped[0];
  const last = unwrapped.at(-1);
  if (first === undefined || last === undefined || Math.abs(last[0] - first[0]) < 180) {
    return unwrapped;
  }
  const pole = center.lat >= 0 ? 90 : -90;
  return [...unwrapped, [last[0], pole], [first[0], pole], first];
}

export function rayLine(from: LatLon, bearingDeg: number, lengthM: number): Lnglat[] {
  return unwrapTrail([lnglat(from), destination(from, bearingDeg, lengthM)]);
}
