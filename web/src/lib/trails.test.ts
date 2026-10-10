import { afterEach, describe, expect, it } from "vitest";
import {
  clearTrails,
  dropTrail,
  emptyTrail,
  extendTrail,
  geoPosition,
  type LonLat,
  offsetM,
  recordTrail,
  TRAIL_CAPACITY,
  TRAIL_TOLERANCE_M,
  type Trail,
  trailLine,
  trailsOf,
} from "./trails";

const METRE_LAT = 1 / 111_195;

function north(metres: number, lon = 0): LonLat {
  return [lon, metres * METRE_LAT];
}

function walk(points: readonly LonLat[]): Trail {
  const trail = emptyTrail();
  points.forEach((point, at) => extendTrail(trail, point, at));
  return trail;
}

describe("extendTrail", () => {
  it("folds a straight run into one segment", () => {
    const trail = walk(Array.from({ length: 50 }, (_, step) => north(step * 100)));
    expect(trail.points).toEqual([north(0), north(4_900)]);
  });

  it("keeps a corner", () => {
    const trail = walk([north(0), north(1_000), north(2_000), north(2_000, 0.03)]);
    expect(trail.points).toEqual([north(0), north(2_000), north(2_000, 0.03)]);
  });

  it("follows a slow turn within tolerance", () => {
    const arc = Array.from({ length: 90 }, (_, step): LonLat => {
      const angle = (step * Math.PI) / 180;
      return [Math.sin(angle) * 0.1, (1 - Math.cos(angle)) * 0.1];
    });
    const trail = walk(arc);
    expect(trail.points.length).toBeGreaterThan(2);
    expect(trail.points.length).toBeLessThan(arc.length);
    for (const point of arc) {
      expect(distanceToLine(trail.points, point)).toBeLessThan(TRAIL_TOLERANCE_M * 2);
    }
  });

  it("ignores jitter around a moored target", () => {
    const trail = walk([north(0), north(5), north(-3), north(8)]);
    expect(trail.points).toEqual([north(0)]);
  });

  it("starts a new segment when the target turns back", () => {
    const trail = walk([north(0), north(1_000), north(500)]);
    expect(trail.points).toEqual([north(0), north(1_000), north(500)]);
  });

  it("skips records older than the last one", () => {
    const trail = emptyTrail();
    extendTrail(trail, north(0), 10);
    expect(extendTrail(trail, north(1_000), 5)).toBe(false);
    expect(trail.points).toEqual([north(0)]);
  });

  it("trims the oldest points past capacity", () => {
    const zigzag = Array.from({ length: TRAIL_CAPACITY + 10 }, (_, step) =>
      north(step * 100, step % 2 === 0 ? 0 : 0.01),
    );
    const trail = walk(zigzag);
    expect(trail.points.length).toBeLessThanOrEqual(TRAIL_CAPACITY);
    expect(trail.points.at(-1)).toEqual(zigzag.at(-1));
  });

  it("hands out a fresh line after each change", () => {
    const trail = walk([north(0), north(1_000)]);
    const first = trailLine(trail);
    expect(trailLine(trail)).toBe(first);
    extendTrail(trail, north(1_000, 0.03), 9);
    expect(trailLine(trail)).not.toBe(first);
    expect(first).toEqual([north(0), north(1_000)]);
  });
});

describe("offsetM", () => {
  it("measures across the antimeridian the short way", () => {
    const [x] = offsetM([179.99, 0], [-179.99, 0]);
    expect(x).toBeGreaterThan(0);
    expect(x).toBeLessThan(3_000);
  });
});

describe("geoPosition", () => {
  it("rejects missing and out of range coordinates", () => {
    expect(geoPosition(48.1, 11.5)).toEqual([11.5, 48.1]);
    expect(geoPosition(null, 11.5)).toBeNull();
    expect(geoPosition(91, 181)).toBeNull();
    expect(geoPosition(Number.NaN, 0)).toBeNull();
  });
});

describe("trail index", () => {
  afterEach(clearTrails);

  it("records positions per target and forgets dropped ones", () => {
    recordTrail("adsb", "abc", event(48, 11), 1);
    recordTrail("adsb", "abc", event(null, null), 2);
    recordTrail("adsb", "abc", event(48.1, 11), 3);
    expect(trailsOf("adsb").get("abc")?.points).toEqual([
      [11, 48],
      [11, 48.1],
    ]);
    dropTrail("adsb", "abc");
    expect(trailsOf("adsb").size).toBe(0);
  });

  it("keeps no trail for decoders without a position", () => {
    recordTrail("pocsag", "1", { kind: "pocsag", data: { address: 1 } } as never, 1);
    expect(trailsOf("pocsag").size).toBe(0);
  });
});

function event(lat: number | null, lon: number | null) {
  return { kind: "adsb", data: { df: 17, icao: "abc", raw: "8d", lat, lon } } as const;
}

function distanceToLine(line: readonly LonLat[], point: LonLat): number {
  let best = Infinity;
  for (let index = 1; index < line.length; index += 1) {
    const from = line[index - 1];
    const to = line[index];
    if (from === undefined || to === undefined) {
      continue;
    }
    const [bx, by] = offsetM(from, to);
    const [px, py] = offsetM(from, point);
    const t = Math.max(0, Math.min(1, (px * bx + py * by) / (bx * bx + by * by)));
    best = Math.min(best, Math.hypot(px - t * bx, py - t * by));
  }
  return best;
}
