import { describe, expect, it } from "vitest";
import type { StationOf } from "../decoded";
import type { SondeForecast } from "../types";
import { TARGET_MAX_AGE_MS } from "./layers";
import { forecastCollection } from "./sondeForecast";

const NOW = Date.parse("2026-10-12T12:00:00Z");

const FORECAST: SondeForecast = {
  burst: { lat: 48.2, lon: 11.4 },
  burst_altitude_m: 31_000,
  burst_seen: true,
  landing: { lat: 48.1, lon: 11.9 },
  landing_in_s: 900,
  path: [
    { lat: 48.0, lon: 179.5 },
    { lat: 48.1, lon: -179.5 },
  ],
};

function sonde(forecast: SondeForecast | undefined, lastSeen = NOW): StationOf<"radiosonde"> {
  return {
    kind: "radiosonde",
    id: "S1",
    event: {
      kind: "radiosonde",
      data: { sonde: "rs41", serial: "S1", errors_corrected: 0, forecast },
    },
    lastSeen,
    freqHz: 403_000_000,
    deviceSet: 0,
    channel: 0,
    frames: 1,
  };
}

describe("forecastCollection", () => {
  it("draws the path unwrapped across the antimeridian with burst and landing marks", () => {
    const collection = forecastCollection([sonde(FORECAST)], NOW);
    expect(collection.features.map((feature) => feature.properties.mark)).toEqual([
      "path",
      "burst",
      "landing",
    ]);
    expect(collection.features[0]?.geometry.coordinates).toEqual([
      [179.5, 48.0],
      [180.5, 48.1],
    ]);
    expect(collection.features[2]?.geometry.coordinates).toEqual([11.9, 48.1]);
  });

  it("skips sondes without a forecast and stale ones", () => {
    const stale = NOW - TARGET_MAX_AGE_MS - 1;
    expect(forecastCollection([sonde(undefined), sonde(FORECAST, stale)], NOW).features).toEqual(
      [],
    );
  });
});
