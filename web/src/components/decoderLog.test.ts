import { describe, expect, it } from "vitest";
import type { DecodedRecord, DecoderEvent, DecoderLogEntry, Presentation } from "../lib/types";
import {
  buildRows,
  callsignTitle,
  clampColumnWidth,
  collectLive,
  DEFAULT_LOG_FILTER,
  defaultColumnWidths,
  droppedNotice,
  FLEX_COLUMN,
  isFiltered,
  LOG_COLUMNS,
  type LogFilter,
  liveRow,
  logDownloads,
  MAX_COLUMN_WIDTH,
  MIN_COLUMN_WIDTH,
  matchesFilter,
  readColumnWidths,
  resizeColumn,
  rowSignature,
  storedRow,
  toQuery,
  totalColumnWidth,
  writeColumnWidths,
} from "./decoderLog";

const LOG = "decoder_log:1";

const adsb: DecoderEvent = {
  kind: "adsb",
  data: { icao: "3c6444", df: 17, callsign: " DLH123 ", altitude_ft: 35_000, raw: "8d3c6444" },
};

const ais: DecoderEvent = {
  kind: "ais",
  data: {
    mmsi: 211_234_560,
    msg_type: 1,
    ais_channel: "A",
    nmea: "!AIVDM,1,1,,A,x,0*00",
    name: " NORDLICHT ",
    lat: 53.551_2,
    lon: 9.993_7,
  },
};

const PLANE_VIEW: Presentation = {
  summary: "3C6444 · DLH123 · 35000 ft",
  entity: "3C6444",
  fields: [{ label: "ICAO", value: { type: "text", value: "3C6444" } }],
};

const SHIP_VIEW: Presentation = { summary: "211234560 · NORDLICHT", entity: "211234560" };

function entry(over: Partial<DecoderLogEntry> = {}): DecoderLogEntry {
  return {
    id: 1,
    at: "2026-08-09T12:00:00Z",
    kind: "adsb",
    station: "3C6444",
    summary: "3C6444 · DLH123",
    freq_hz: 1_090_000_000,
    device_set: 0,
    channel: 0,
    event: adsb,
    origin: { node: "adsb", transmission: 0 },
    ...over,
  };
}

function record(over: Partial<DecodedRecord> = {}): DecodedRecord {
  return {
    at: "2026-08-09T12:00:01Z",
    event: adsb,
    freq_hz: 1_090_000_000,
    device_set: 0,
    channel: 0,
    origin: { node: "adsb", transmission: 0 },
    sinks: [LOG, "export:1"],
    view: PLANE_VIEW,
    ...over,
  };
}

function filter(over: Partial<LogFilter> = {}): LogFilter {
  return { ...DEFAULT_LOG_FILTER, ...over };
}

describe("toQuery", () => {
  const wires = { sink: LOG, wired: true };

  it("drops empty selects so a cleared filter is one query key, not two", () => {
    expect(toQuery(filter(), wires)).toEqual({ limit: 500, sink: LOG });
    expect(toQuery(filter({ q: "   " }), wires)).toEqual({ limit: 500, sink: LOG });
  });

  it("carries every set field, trimmed", () => {
    expect(toQuery(filter({ q: " nord ", limit: 100 }), wires)).toEqual({
      q: "nord",
      limit: 100,
      sink: LOG,
    });
  });

  it("asks by the sink node even when nothing is wired in yet", () => {
    expect(toQuery(filter(), { sink: LOG, wired: false })).toEqual({ limit: 500, sink: LOG });
  });
});

describe("isFiltered", () => {
  it("ignores the row limit", () => {
    expect(isFiltered(filter({ limit: 100 }))).toBe(false);
    expect(isFiltered(filter({ q: " x " }))).toBe(true);
  });
});

describe("matchesFilter", () => {
  it("searches station and summary case-insensitively", () => {
    expect(matchesFilter(record(), filter({ q: "DLH" }))).toBe(true);
    expect(matchesFilter(record(), filter({ q: "3c6444" }))).toBe(true);
    expect(matchesFilter(record(), filter({ q: "nordlicht" }))).toBe(false);
    expect(matchesFilter(record(), filter())).toBe(true);
  });
});

describe("collectLive", () => {
  const records = [
    record({ at: "2026-08-09T12:00:01Z" }),
    record({ at: "2026-08-09T12:00:03Z" }),
    record({ at: "2026-08-09T12:00:02Z", event: ais, view: SHIP_VIEW }),
  ];

  it("lists the feed newest first", () => {
    expect(collectLive(records, filter(), 0).map((r) => r.at)).toEqual([
      "2026-08-09T12:00:03Z",
      "2026-08-09T12:00:02Z",
      "2026-08-09T12:00:01Z",
    ]);
  });

  it("honours the filter, the cap and the last clear", () => {
    expect(collectLive(records, filter({ q: "nordlicht" }), 0)).toHaveLength(1);
    expect(collectLive(records, filter(), 0, 2)).toHaveLength(2);
    expect(collectLive(records, filter(), Date.parse("2026-08-09T12:00:02Z"))).toHaveLength(1);
  });
});

describe("buildRows", () => {
  it("puts live rows above the stored page and marks them", () => {
    const rows = buildRows([entry()], [record()]);
    expect(rows.map((r) => r.live)).toEqual([true, false]);
    expect(rows[0]?.summary).toBe("3C6444 · DLH123 · 35000 ft");
  });

  it("orders the tail and the stored page as one table", () => {
    const rows = buildRows(
      [entry({ id: 2, at: "2026-08-09T12:00:04Z" }), entry({ at: "2026-08-09T12:00:00Z" })],
      [record({ at: "2026-08-09T12:00:05Z" }), record({ at: "2026-08-09T12:00:02Z" })],
    );
    expect(rows.map((r) => r.at)).toEqual([
      "2026-08-09T12:00:05Z",
      "2026-08-09T12:00:04Z",
      "2026-08-09T12:00:02Z",
      "2026-08-09T12:00:00Z",
    ]);
  });

  it("drops a live frame the stored page already carries", () => {
    const stored = entry({ at: "2026-08-09T12:00:01Z", view: PLANE_VIEW });
    expect(buildRows([stored], [record()])).toHaveLength(1);
    expect(buildRows([stored], [record()])[0]?.live).toBe(false);
  });

  it("gives a live frame and its stored copy the same signature", () => {
    const stored = storedRow(entry({ at: "2026-08-09T12:00:01Z", view: PLANE_VIEW }));
    expect(rowSignature(stored)).toBe(rowSignature(liveRow(record())));
  });

  it("keys rows uniquely even when two identical frames arrive at the same instant", () => {
    const rows = buildRows([entry(), entry({ id: 2 })], [record(), record()]);
    expect(new Set(rows.map((r) => r.key)).size).toBe(rows.length);
  });
});

describe("row projection", () => {
  it("reads a stored row from its presented view", () => {
    expect(storedRow(entry({ view: SHIP_VIEW }))).toMatchObject({
      key: "stored:1",
      station: "211234560",
      summary: "211234560 · NORDLICHT",
      live: false,
    });
    expect(storedRow(entry({ station: null }))).toMatchObject({
      station: null,
      summary: "3C6444 · DLH123",
    });
  });

  it("carries the resolved country of a callsign", () => {
    const germany = {
      call: "DL1ABC",
      country: "Fed. Rep. of Germany",
      prefix: "DL",
      continent: "EU",
      cq_zone: 14,
      itu_zone: 28,
    };
    expect(storedRow(entry({ callsign: germany })).callsign).toEqual(germany);
    expect(liveRow(record({ callsign: germany })).callsign).toEqual(germany);
    expect(liveRow(record()).callsign).toBeNull();
    expect(callsignTitle(germany)).toBe("Fed. Rep. of Germany, EU, CQ 14, ITU 28");
  });

  it("takes fields and the clip from the view", () => {
    const row = liveRow(
      record({
        view: { ...PLANE_VIEW, clip: { url: "/api/calls/1/audio", media_type: "audio/wav" } },
      }),
    );
    expect(row.fields).toEqual(PLANE_VIEW.fields);
    expect(row.clip?.url).toBe("/api/calls/1/audio");
  });
});

describe("droppedNotice", () => {
  it("stays silent only when nothing was lost", () => {
    expect(droppedNotice(0, 0)).toBeNull();
    expect(droppedNotice(1, 0)).toBe("1 live frame dropped");
    expect(droppedNotice(0, 12)).toBe("12 frames never reached the log");
    expect(droppedNotice(2, 12)).toBe("2 live frames dropped · 12 frames never reached the log");
  });
});

function fakeStorage(): Storage {
  const map = new Map<string, string>();
  return {
    get length() {
      return map.size;
    },
    clear: () => map.clear(),
    getItem: (key: string) => map.get(key) ?? null,
    key: (index: number) => [...map.keys()][index] ?? null,
    removeItem: (key: string) => map.delete(key),
    setItem: (key: string, value: string) => map.set(key, value),
  };
}

function useStorage(store: Storage | undefined): void {
  Object.defineProperty(globalThis, "localStorage", {
    value: store,
    configurable: true,
    writable: true,
  });
}

describe("column widths", () => {
  it("starts from the declared defaults", () => {
    const widths = defaultColumnWidths();
    expect(Object.keys(widths)).toEqual(LOG_COLUMNS.map((column) => column.key));
    expect(totalColumnWidth(widths)).toBe(
      LOG_COLUMNS.reduce((sum, column) => sum + column.width, 0),
    );
  });

  it("clamps to the allowed range and rounds to whole pixels", () => {
    expect(clampColumnWidth(MIN_COLUMN_WIDTH - 40)).toBe(MIN_COLUMN_WIDTH);
    expect(clampColumnWidth(MAX_COLUMN_WIDTH + 40)).toBe(MAX_COLUMN_WIDTH);
    expect(clampColumnWidth(120.4)).toBe(120);
    expect(clampColumnWidth(Number.NaN)).toBe(MIN_COLUMN_WIDTH);
  });

  it("resizes one column without touching the others", () => {
    const widths = defaultColumnWidths();
    const next = resizeColumn(widths, "station", 240);
    expect(next.station).toBe(240);
    expect(next.summary).toBe(widths.summary);
    expect(widths.station).toBe(defaultColumnWidths().station);
  });

  it("round-trips through storage", () => {
    useStorage(fakeStorage());
    writeColumnWidths(resizeColumn(defaultColumnWidths(), "kind", 200));
    expect(readColumnWidths().kind).toBe(200);
  });

  it("keeps the flex column off storage so it always fills the panel", () => {
    const store = fakeStorage();
    useStorage(store);
    store.setItem(
      "sdrmm.decoderLog.columns",
      JSON.stringify({ ...defaultColumnWidths(), [FLEX_COLUMN]: 900 }),
    );
    expect(readColumnWidths()[FLEX_COLUMN]).toBe(defaultColumnWidths()[FLEX_COLUMN]);
    expect(totalColumnWidth(readColumnWidths())).toBe(totalColumnWidth(defaultColumnWidths()));
    useStorage(undefined);
  });

  it("falls back to defaults on missing, corrupt or bogus storage", () => {
    useStorage(undefined);
    expect(readColumnWidths()).toEqual(defaultColumnWidths());

    const store = fakeStorage();
    useStorage(store);
    store.setItem("sdrmm.decoderLog.columns", "{not json");
    expect(readColumnWidths()).toEqual(defaultColumnWidths());

    store.setItem(
      "sdrmm.decoderLog.columns",
      JSON.stringify({ kind: "wide", station: 9000, bogus: 12 }),
    );
    const widths = readColumnWidths();
    expect(widths.kind).toBe(defaultColumnWidths().kind);
    expect(widths.station).toBe(MAX_COLUMN_WIDTH);
    expect(Object.keys(widths)).toEqual(LOG_COLUMNS.map((column) => column.key));

    useStorage(undefined);
  });
});

describe("logDownloads", () => {
  it("offers both formats over the same filter", () => {
    const choices = logDownloads({ limit: 200, sink: "export:1", q: "wx" });
    expect(choices.map((choice) => choice.label)).toEqual(["CSV", "JSON"]);
    for (const choice of choices) {
      expect(choice.href).toContain("sink=export%3A1");
      expect(choice.href).toContain("q=wx");
    }
    expect(choices[0]?.href).toContain("csv");
    expect(choices[1]?.href).toContain("json");
  });
});
