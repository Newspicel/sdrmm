import { describe, expect, it } from "vitest";
import type { SinkGroup } from "../lib/sinks";
import type { Field, SortKey } from "../lib/types";
import { sinkEntity, sinkRecord } from "../test/sinks";
import {
  fieldNumber,
  mergeFields,
  meterLevel,
  newestField,
  nextSort,
  recordRows,
  sortRows,
  tableRows,
  trendLine,
  trendRange,
  trendSeries,
  type ViewRow,
} from "./viewData";

const EVENT = { kind: "probe", data: {} };

function text(key: string, value: string, label = key): Field {
  return { key, label, value: { type: "text", value } };
}

function num(key: string, value: number, label = key): Field {
  return { key, label, value: { type: "number", value: { value, unit: "db", digits: 1 } } };
}

function record(fields: Field[], at: string, entity?: string) {
  return sinkRecord("probe", EVENT, { summary: "s", fields, ...(entity ? { entity } : {}) }, at);
}

function row(id: string, fields: Field[], seenMs = 0): ViewRow {
  return { id, name: id, summary: id, seenMs, fields };
}

describe("table rows", () => {
  it("merges the newest value of each field per key value", () => {
    const rows = recordRows(
      [
        record([text("call", "A"), num("snr", 9)], "2026-09-21T10:00:03Z"),
        record([text("call", "B"), num("snr", 4)], "2026-09-21T10:00:02Z"),
        record([text("call", "A"), num("snr", 1), text("grid", "JO62")], "2026-09-21T10:00:01Z"),
        record([num("snr", 7)], "2026-09-21T10:00:00Z"),
      ],
      "call",
    );
    expect(rows.map((one) => one.id)).toEqual([
      '{"type":"text","value":"A"}',
      '{"type":"text","value":"B"}',
    ]);
    expect(rows[0]?.fields.map((field) => field.key)).toEqual(["call", "snr", "grid"]);
    expect(fieldNumber(rows[0]?.fields[1])).toBe(9);
    expect(rows[0]?.seenMs).toBe(Date.parse("2026-09-21T10:00:03Z"));
  });

  it("keys by the presentation entity without a key field", () => {
    const rows = recordRows(
      [record([num("snr", 3)], "2026-09-21T10:00:01Z", "E1"), record([], "2026-09-21T10:00:00Z")],
      null,
    );
    expect(rows.map((one) => one.name)).toEqual(["E1"]);
  });

  it("prefers the sink's entities while rows key by entity", () => {
    const group: SinkGroup = {
      source: "probe",
      kind: "probe",
      entities: [sinkEntity("probe", "probe", "X", EVENT, { summary: "x", label: "Ex" })],
      records: [record([], "2026-09-21T10:00:00Z", "Y")],
    };
    expect(tableRows(group, null).map((one) => one.name)).toEqual(["Ex"]);
    expect(tableRows(group, "call")).toEqual([]);
  });

  it("keeps newer fields when merging", () => {
    expect(mergeFields([num("a", 1)], [num("a", 2), num("b", 3)]).map(fieldNumber)).toEqual([1, 3]);
  });
});

describe("sorting", () => {
  const rows = [
    row("low", [num("snr", 2)], 3),
    row("none", [], 2),
    row("high", [num("snr", 20)], 1),
  ];
  const ids = (sort: SortKey | null) => sortRows(rows, sort).map((one) => one.id);

  it("orders by a field either way, rows lacking it last", () => {
    expect(ids({ field: "snr", descending: false })).toEqual(["low", "high", "none"]);
    expect(ids({ field: "snr", descending: true })).toEqual(["high", "low", "none"]);
    expect(ids(null)).toEqual(["low", "none", "high"]);
  });

  it("compares text naturally", () => {
    const named = [row("b", [text("n", "ch10")]), row("a", [text("n", "ch9")])];
    expect(sortRows(named, { field: "n" }).map((one) => one.id)).toEqual(["a", "b"]);
  });

  it("flips the clicked column, sorts another ascending", () => {
    expect(nextSort(null, "snr")).toEqual({ field: "snr", descending: false });
    expect(nextSort({ field: "snr", descending: false }, "snr")).toEqual({
      field: "snr",
      descending: true,
    });
    expect(nextSort({ field: "snr", descending: true }, "call")).toEqual({
      field: "call",
      descending: false,
    });
  });
});

describe("meter", () => {
  it("places a value on its range, clamped", () => {
    expect(meterLevel(15, 10, 20)).toBe(0.5);
    expect(meterLevel(-5, 0, 10)).toBe(0);
    expect(meterLevel(50, 0, 10)).toBe(1);
    expect(meterLevel(5, 10, 10)).toBe(0);
  });

  it("reads the newest record carrying the field", () => {
    const records = [
      record([text("call", "A")], "2026-09-21T10:00:02Z"),
      record([num("snr", 4)], "2026-09-21T10:00:01Z"),
      record([num("snr", 8)], "2026-09-21T10:00:00Z"),
    ];
    expect(fieldNumber(newestField(records, "snr"))).toBe(4);
    expect(newestField(records, "gone")).toBeUndefined();
    expect(fieldNumber(text("call", "A"))).toBeNull();
  });
});

describe("trend", () => {
  const now = Date.parse("2026-09-21T10:01:00Z");
  const records = [
    record([num("snr", 6, "SNR")], "2026-09-21T10:00:50Z"),
    record([num("snr", 2, "SNR"), text("call", "A")], "2026-09-21T10:00:30Z"),
    record([num("snr", 99, "SNR")], "2026-09-21T09:59:00Z"),
  ];

  it("keeps numeric values inside the window, oldest first", () => {
    const [series] = trendSeries(records, ["snr"], 60, now);
    expect(series?.label).toBe("SNR");
    expect(series?.points.map((point) => point.value)).toEqual([2, 6]);
    expect(trendSeries(records, ["call"], 60, now)[0]?.points).toEqual([]);
  });

  it("spans every series and widens a flat one", () => {
    const series = trendSeries(records, ["snr"], 60, now);
    expect(trendRange(series)).toEqual({ lo: 2, hi: 6 });
    expect(trendRange([{ key: "a", label: "a", points: [{ atMs: 0, value: 3 }] }])).toEqual({
      lo: 2,
      hi: 4,
    });
    expect(trendRange([])).toBeNull();
  });

  it("maps time across and value up the box", () => {
    const points = [
      { atMs: now - 60_000, value: 0 },
      { atMs: now, value: 10 },
    ];
    const line = trendLine(
      points,
      { lo: 0, hi: 10 },
      { windowS: 60, nowMs: now },
      {
        width: 100,
        height: 50,
      },
    );
    expect(line).toBe("0.0,50.0 100.0,0.0");
  });
});
