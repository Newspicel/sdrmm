import { describe, expect, it } from "vitest";
import type { DecoderLogEntry, DecoderLogGroup } from "../lib/types";
import { formatAirtime, groupOf, groupRows, hasAirtime, sortGroups, viewOf } from "./logGroups";

function entry(id: number, freqHz: number, station: string | null): DecoderLogEntry {
  return {
    id,
    at: "2026-10-05T10:00:00Z",
    device_set: 0,
    channel: 0,
    kind: "adsb",
    freq_hz: freqHz,
    station,
    summary: `row ${id}`,
    event: { kind: "adsb", data: { icao: station ?? "", df: 17, raw: "" } },
  };
}

function group(
  id: number,
  freqHz: number,
  count: number,
  lastAt: string,
  airtimeMs?: number,
): DecoderLogGroup {
  return {
    count,
    first_at: "2026-10-05T09:00:00Z",
    last_at: lastAt,
    airtime_ms: airtimeMs,
    latest: entry(id, freqHz, `S${id}`),
  };
}

const groups = [
  group(1, 145_600_000, 2, "2026-10-05T10:00:30Z", 3_000),
  group(2, 145_500_000, 9, "2026-10-05T10:00:10Z"),
  group(3, 145_700_000, 4, "2026-10-05T10:00:20Z"),
];

describe("grouped decoder log", () => {
  it("maps the stored list setting to a view and back", () => {
    expect(viewOf(undefined)).toBe("list");
    expect(viewOf("station")).toBe("station");
    expect(groupOf("list")).toBeNull();
    expect(groupOf("frequency")).toBe("frequency");
  });

  it("labels a frequency group by its frequency and a station group by its station", () => {
    expect(groupRows(groups, "frequency")[0]?.label).toBe("145.6000 MHz");
    expect(groupRows(groups, "station")[0]?.label).toBe("S1");
    expect(
      groupRows([{ ...group(4, 1, 1, ""), latest: entry(4, 1, null) }], "station")[0]?.label,
    ).toBe("-");
  });

  it("sorts by frequency, hits or last heard", () => {
    const rows = groupRows(groups, "frequency");
    const order = (sort: "key" | "count" | "last") =>
      sortGroups(rows, sort, "frequency").map((row) => row.latest.station);
    expect(order("key")).toEqual(["S2", "S1", "S3"]);
    expect(order("count")).toEqual(["S2", "S3", "S1"]);
    expect(order("last")).toEqual(["S1", "S3", "S2"]);
  });

  it("shows airtime only when a group carries any", () => {
    const rows = groupRows(groups, "frequency");
    expect(hasAirtime(rows)).toBe(true);
    expect(hasAirtime(rows.slice(1))).toBe(false);
    expect(formatAirtime(3_000)).toBe("3.0 s");
    expect(formatAirtime(null)).toBe("-");
  });
});
