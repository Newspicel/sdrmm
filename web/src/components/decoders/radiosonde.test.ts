import { describe, expect, it } from "vitest";
import type { DecodedRecordOf, DecoderEventOf } from "../../lib/types";
import { NOW, record } from "../../test/decoderRecords";
import { BURST_DROP_M, radiosondeLog, radiosondeStations } from "./radiosonde";

function sondeAt(
  offsetS: number,
  data: Partial<DecoderEventOf<"radiosonde">["data"]>,
): DecodedRecordOf<"radiosonde"> {
  return record(
    "radiosonde",
    { sonde: "rs41", serial: "S1", errors_corrected: 0, ...data },
    { at: new Date(NOW + offsetS * 1000).toISOString(), freq_hz: 403_000_000 },
  );
}

describe("radiosondeStations", () => {
  it("keeps the latest value of each field per serial", () => {
    const sondes = radiosondeStations([
      sondeAt(2, { frame: 3, altitude_m: 1_200 }),
      sondeAt(1, { frame: 2, temperature_c: 5, humidity_pct: 70 }),
      sondeAt(0, { serial: "S2", sonde: "dfm", altitude_m: 300 }),
    ]);
    expect(sondes.map((s) => s.serial)).toEqual(["S1", "S2"]);
    expect(sondes[0]).toMatchObject({
      frame: 3,
      altitudeM: 1_200,
      temperatureC: 5,
      humidityPct: 70,
      frames: 2,
      burst: false,
    });
  });

  it("flags a burst once altitude falls well below the peak", () => {
    const peak = 30_000;
    const rising = radiosondeStations([
      sondeAt(1, { altitude_m: peak - BURST_DROP_M + 1 }),
      sondeAt(0, { altitude_m: peak }),
    ]);
    expect(rising[0]?.burst).toBe(false);
    const fallen = radiosondeStations([
      sondeAt(3, { altitude_m: 2_000 }),
      sondeAt(2, { altitude_m: peak - BURST_DROP_M - 1 }),
      sondeAt(1, { altitude_m: peak }),
      sondeAt(0, { altitude_m: 29_000 }),
    ]);
    expect(fallen[0]).toMatchObject({ burst: true, maxAltitudeM: peak, altitudeM: 2_000 });
  });
});

describe("radiosondeLog", () => {
  it("lists one sonde's newest frames first, up to the limit", () => {
    const records = [
      sondeAt(3, { frame: 4 }),
      sondeAt(2, { serial: "S2", frame: 9 }),
      sondeAt(1, { frame: 2 }),
      sondeAt(0, { frame: 1 }),
    ];
    expect(radiosondeLog(records, "S1", 2).map((r) => r.event.data.frame)).toEqual([4, 2]);
  });
});
