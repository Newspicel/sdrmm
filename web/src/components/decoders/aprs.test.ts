import { describe, expect, it } from "vitest";
import type { DecodedRecordOf, DecoderEventOf } from "../../lib/types";
import { NOW, record } from "../../test/decoderRecords";
import { aprsWeatherStations } from "./aprs";

function weatherAt(
  offsetS: number,
  source: string,
  weather: DecoderEventOf<"aprs">["data"]["weather"],
): DecodedRecordOf<"aprs"> {
  return record(
    "aprs",
    { source, destination: "APRS", info: "_", tnc2: `${source}>APRS:_`, weather },
    { at: new Date(NOW + offsetS * 1000).toISOString() },
  );
}

describe("aprsWeatherStations", () => {
  it("keeps latest values and extremes per station", () => {
    const stations = aprsWeatherStations([
      weatherAt(3, "WX1", { temperature_c: 14, wind_gust_ms: 3 }),
      weatherAt(2, "WX2", { temperature_c: 20 }),
      weatherAt(1, "WX1", { temperature_c: 9, wind_gust_ms: 11, pressure_hpa: 1010 }),
      weatherAt(0, "WX1", { temperature_c: 12, humidity_pct: 80 }),
    ]);
    expect(stations.map((s) => s.source)).toEqual(["WX1", "WX2"]);
    expect(stations[0]).toMatchObject({
      weather: { temperature_c: 14, wind_gust_ms: 3, pressure_hpa: 1010, humidity_pct: 80 },
      minTemperatureC: 9,
      maxTemperatureC: 14,
      maxGustMs: 11,
      reports: 3,
    });
  });

  it("skips packets without weather", () => {
    expect(aprsWeatherStations([weatherAt(0, "DL1ABC", null)])).toEqual([]);
  });

  it("leaves extremes empty when never reported", () => {
    expect(aprsWeatherStations([weatherAt(0, "WX", { humidity_pct: 50 })])[0]).toMatchObject({
      minTemperatureC: null,
      maxTemperatureC: null,
      maxGustMs: null,
    });
  });
});
