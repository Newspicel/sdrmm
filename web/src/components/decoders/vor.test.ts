import { describe, expect, it } from "vitest";
import { record } from "../../test/decoderRecords";
import { latestVorReadings, multiVorFix } from "./vor";

function vorReading(stationName: string, lat: number, lon: number, radial: number) {
  return {
    station: stationName,
    station_lat: lat,
    station_lon: lon,
    magnetic_declination_deg: 0,
    radial_deg: radial,
    variable_phase_deg: 0,
    reference_phase_deg: 0,
    signal_db: -12,
    confidence: 1,
  };
}

describe("multi-VOR fixes", () => {
  it("intersects two non-parallel radials", () => {
    const records = [
      record("vor", vorReading("A", 0, 0, 45)),
      record("vor", vorReading("B", 0, 2, 315), { channel: 1 }),
    ];
    const fix = multiVorFix(records);
    expect(fix).not.toBeNull();
    expect(fix?.lat).toBeCloseTo(1, 3);
    expect(fix?.lon).toBeCloseTo(1, 3);
    expect(fix?.residualKm).toBeLessThan(0.001);
    expect(fix?.stations).toBe(2);
  });

  it("intersects radials across the antimeridian", () => {
    const records = [
      record("vor", vorReading("A", 0, 179.5, 45)),
      record("vor", vorReading("B", 0, -178.5, 315), { channel: 1 }),
    ];
    const fix = multiVorFix(records);
    expect(fix).not.toBeNull();
    expect(fix?.lat).toBeCloseTo(1, 3);
    expect(fix?.lon).toBeCloseTo(-179.5, 3);
    expect(fix?.residualKm).toBeLessThan(0.001);
  });

  it("uses the newest reading from each station", () => {
    const newest = record("vor", vorReading("A", 0, 0, 45), {
      at: "2026-08-09T12:00:01Z",
    });
    const older = record("vor", vorReading("A", 0, 0, 90));
    expect(latestVorReadings([older, newest])).toEqual([newest]);
    expect(multiVorFix([newest])).toBeNull();
  });
});
