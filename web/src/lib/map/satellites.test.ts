import { describe, expect, it } from "vitest";
import { greatCircleKm } from "../propagation";
import type { SatelliteStatus } from "../types";
import {
  FOOTPRINT_POINTS,
  footprintRing,
  satelliteCollections,
  satelliteLabel,
} from "./satellites";

function status(over: Partial<SatelliteStatus> = {}): SatelliteStatus {
  return {
    node: "sat-1",
    name: "ISS (ZARYA)",
    visible: true,
    ground: { at: { lat: 10, lon: 20 }, altitude_km: 420, footprint_km: 2_250 },
    ground_track: [
      { lat: 0, lon: 170 },
      { lat: 5, lon: -175 },
    ],
    ...over,
  };
}

describe("footprintRing", () => {
  it("closes a ring the footprint radius away from the sub-point", () => {
    const ring = footprintRing({ lat: 10, lon: 20 }, 2_250);
    expect(ring).toHaveLength(FOOTPRINT_POINTS + 1);
    for (const [lon, lat] of ring) {
      expect(greatCircleKm([10, 20], [lat, lon])).toBeCloseTo(2_250, 0);
    }
    expect(ring[0]?.[0]).toBeCloseTo(ring.at(-1)?.[0] ?? Number.NaN, 6);
  });

  it("runs a ring around the pole along the top edge", () => {
    const ring = footprintRing({ lat: 80, lon: 0 }, 2_500);
    expect(ring.at(-2)?.[1]).toBe(90);
    expect(ring.at(-3)?.[1]).toBe(90);
    expect(ring.at(-1)).toEqual(ring[0]);
  });
});

describe("satelliteCollections", () => {
  it("places the sub-point, footprint and an unwrapped ground track", () => {
    const collections = satelliteCollections([status()]);
    expect(collections.point.features[0]?.geometry.coordinates).toEqual([20, 10]);
    expect(collections.point.features[0]?.properties).toEqual({
      id: "sat-1",
      label: "ISS (ZARYA)",
      visible: true,
    });
    expect(collections.footprint.features).toHaveLength(1);
    expect(collections.track.features[0]?.geometry.coordinates).toEqual([
      [170, 0],
      [185, 5],
    ]);
  });

  it("leaves out a satellite with no position yet", () => {
    const collections = satelliteCollections([
      status({ ground: undefined, ground_track: undefined }),
    ]);
    expect(collections.point.features).toEqual([]);
    expect(collections.footprint.features).toEqual([]);
    expect(collections.track.features).toEqual([]);
  });
});

describe("satelliteLabel", () => {
  it("falls back from name to catalog number to node", () => {
    expect(satelliteLabel(status({ name: " ", catalog: "25544" }))).toBe("25544");
    expect(satelliteLabel(status({ name: undefined, catalog: undefined }))).toBe("sat-1");
  });
});
