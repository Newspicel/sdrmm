import { describe, expect, it } from "vitest";
import { NOW, record, station } from "../test/decoderRecords";
import {
  ageClass,
  aircraftRow,
  appendTranscript,
  buildTranscript,
  formatAge,
  formatAltitudeFt,
  formatBearing,
  formatClock,
  formatPosition,
  formatSpeedKt,
  isAtBottom,
  latestRecord,
  latestWpm,
  shipRow,
  sortTargets,
  TARGET_MAX_AGE_MS,
  TARGET_STALE_MS,
} from "./decoderViews";

describe("ageing", () => {
  it("dims a target before it disappears", () => {
    expect(ageClass(0)).toBe("text-ink");
    expect(ageClass(TARGET_STALE_MS - 1)).toBe("text-ink");
    expect(ageClass(TARGET_STALE_MS)).toBe("text-ink-dim");
    expect(ageClass(TARGET_MAX_AGE_MS / 2)).toBe("text-ink-dim opacity-45");
  });

  it("formats age as seconds, m:ss then hours", () => {
    expect(formatAge(0)).toBe("0s");
    expect(formatAge(59_400)).toBe("59s");
    expect(formatAge(60_000)).toBe("1:00");
    expect(formatAge(3_599_000)).toBe("59:59");
    expect(formatAge(3_600_000)).toBe("1h00");
    expect(formatAge(-5_000)).toBe("0s");
  });
});

describe("target rows", () => {
  const aircraft = station(
    "adsb",
    "3c6444",
    {
      icao: "3c6444",
      df: 17,
      raw: "8d3c6444",
      callsign: " DLH123 ",
      altitude_ft: 37_000,
      ground_speed_kt: 451.4,
      track_deg: 271.6,
      lat: 52.52,
      lon: 13.405,
    },
    { frames: 42, last_seen: new Date(NOW - 12_000).toISOString() },
  );

  it("projects an aircraft", () => {
    expect(aircraftRow(aircraft, NOW)).toEqual({
      id: "3C6444",
      label: "DLH123",
      primary: "37,000 ft",
      secondary: "451 kt · 272°",
      position: "52.52000, 13.40500",
      ageMs: 12_000,
      frames: 42,
    });
  });

  it("shows GND instead of an altitude and never a negative age", () => {
    const onGround = station(
      "adsb",
      "3c6444",
      { icao: "3c6444", df: 17, raw: "8d", on_ground: true, altitude_ft: 0 },
      { last_seen: new Date(NOW + 500).toISOString() },
    );
    const row = aircraftRow(onGround, NOW);
    expect(row.primary).toBe("GND");
    expect(row.label).toBe("-");
    expect(row.position).toBe("-");
    expect(row.ageMs).toBe(0);
  });

  it("falls back from ship name to call sign", () => {
    const ship = station(
      "ais",
      "211234560",
      {
        mmsi: 211234560,
        msg_type: 1,
        ais_channel: "A",
        nmea: "!AIVDM",
        call_sign: "DEAB",
        sog_kt: 12.4,
        cog_deg: 359.7,
        destination: "HAMBURG",
      },
      { last_seen: new Date(NOW - 90_000).toISOString() },
    );
    expect(shipRow(ship, NOW)).toMatchObject({
      id: "211234560",
      label: "DEAB",
      primary: "12 kt",
      secondary: "0° · HAMBURG",
      position: "-",
      ageMs: 90_000,
    });
  });
});

describe("sortTargets", () => {
  const rows = [
    { id: "3C6444", ageMs: 5_000 },
    { id: "0A0001", ageMs: 40_000 },
    { id: "FFFFFF", ageMs: 1_000 },
  ].map((r) => ({ ...r, label: "", primary: "", secondary: "", position: "", frames: 1 }));

  it("sorts freshest first by age, and reverses on demand", () => {
    expect(sortTargets(rows, "age", false).map((r) => r.id)).toEqual([
      "FFFFFF",
      "3C6444",
      "0A0001",
    ]);
    expect(sortTargets(rows, "age", true).map((r) => r.id)).toEqual(["0A0001", "3C6444", "FFFFFF"]);
  });

  it("sorts identities by length then lexically, so MMSIs order numerically", () => {
    expect(sortTargets(rows, "id", false).map((r) => r.id)).toEqual(["0A0001", "3C6444", "FFFFFF"]);
    const mmsis = [{ id: "9" }, { id: "211234560" }, { id: "100" }].map((r) => ({
      ...r,
      label: "",
      primary: "",
      secondary: "",
      position: "",
      ageMs: 0,
      frames: 1,
    }));
    expect(sortTargets(mmsis, "id", false).map((r) => r.id)).toEqual(["9", "100", "211234560"]);
  });

  it("does not mutate its input", () => {
    const before = rows.map((r) => r.id);
    sortTargets(rows, "id", true);
    expect(rows.map((r) => r.id)).toEqual(before);
  });
});

describe("formatting", () => {
  it("groups altitudes and rounds speeds and bearings", () => {
    expect(formatAltitudeFt(null)).toBe("-");
    expect(formatAltitudeFt(900)).toBe("900 ft");
    expect(formatAltitudeFt(37_000)).toBe("37,000 ft");
    expect(formatAltitudeFt(-1_200)).toBe("−1,200 ft");
    expect(formatSpeedKt(undefined)).toBe("-");
    expect(formatSpeedKt(12.6)).toBe("13 kt");
    expect(formatBearing(null)).toBe("");
    expect(formatBearing(359.7)).toBe("0°");
    expect(formatBearing(-90)).toBe("270°");
  });

  it("needs both halves of a position", () => {
    expect(formatPosition(52.52, null)).toBe("-");
    expect(formatPosition(52.52, 13.405)).toBe("52.52000, 13.40500");
  });

  it("renders a clock, and a placeholder for an unparsable stamp", () => {
    const local = new Date(2026, 7, 9, 12, 34, 56).toISOString();
    expect(formatClock(local)).toBe("12:34:56");
    expect(formatClock("not a date")).toBe("--:--:--");
  });
});

describe("transcripts", () => {
  it("appends until the limit, then drops the head at a line boundary", () => {
    expect(appendTranscript("ab", "cd", 10)).toBe("abcd");
    expect(appendTranscript("abcdef", "gh", 4)).toBe("efgh");
    expect(appendTranscript("one\ntwo\nthree\n", "four\n", 12)).toBe("three\nfour\n");
  });

  it("builds the pane oldest-first from newest-first records", () => {
    const records = [record("rtty", { text: "CQ " }), record("rtty", { text: "TEST " })];
    expect(buildTranscript(records)).toBe("TEST CQ ");
    expect(buildTranscript([])).toBe("");
  });

  it("reports the most recent Morse speed only", () => {
    expect(latestWpm([])).toBeNull();
    expect(
      latestWpm([record("morse", { text: "E", wpm: 22 }), record("morse", { text: "T", wpm: 18 })]),
    ).toBe(22);
  });

  it("treats a near-bottom scroll as bottom", () => {
    expect(isAtBottom({ scrollTop: 900, scrollHeight: 1000, clientHeight: 100 })).toBe(true);
    expect(isAtBottom({ scrollTop: 895, scrollHeight: 1000, clientHeight: 100 })).toBe(true);
    expect(isAtBottom({ scrollTop: 500, scrollHeight: 1000, clientHeight: 100 })).toBe(false);
  });
});

describe("latestRecord", () => {
  it("picks the newest record", () => {
    expect(latestRecord([{ at: "2026-08-09T12:00:01Z" }, { at: "2026-08-09T12:00:03Z" }])).toEqual({
      at: "2026-08-09T12:00:03Z",
    });
    expect(latestRecord([])).toBeNull();
  });
});
