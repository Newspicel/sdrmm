import { describe, expect, it } from "vitest";
import { EMPTY_SINK, groupsOf } from "../lib/sinks";
import type { EventKindEntry, Field } from "../lib/types";
import { sinkEntity, sinkRecord } from "../test/sinks";
import { readoutText, sourceRows } from "./readoutText";

const KINDS: EventKindEntry[] = [
  {
    kind: "probe_text",
    name: "Text",
    noun: "line",
    caps: ["log", "text"],
    hue: "teal",
    lifetime: { type: "instant" },
    views: [],
  },
  {
    kind: "probe_station",
    name: "Station",
    noun: "station",
    caps: ["log", "station"],
    hue: "teal",
    lifetime: { type: "instant" },
    views: [],
  },
  {
    kind: "probe_plain",
    name: "Plain",
    noun: "event",
    caps: ["log"],
    hue: "teal",
    lifetime: { type: "instant" },
    views: [],
  },
];

const named = (source: string): string => source.toUpperCase();

function speed(value: string): Field {
  return { key: "speed", label: "Speed", value: { type: "text", value } };
}

describe("readout text", () => {
  it("copies a transcript as one text", () => {
    const groups = groupsOf({
      ...EMPTY_SINK,
      records: [
        sinkRecord("cw", { kind: "probe_text", data: {} }, { summary: "DE", text: "DE DL1ABC" }),
        sinkRecord("cw", { kind: "probe_text", data: {} }, { summary: "CQ", text: "CQ CQ " }),
      ],
    });
    expect(readoutText(groups, KINDS, named)).toBe("CQ CQ DE DL1ABC");
  });

  it("titles each group and writes a line per station or record, oldest record first", () => {
    const groups = groupsOf({
      ...EMPTY_SINK,
      entities: [
        sinkEntity(
          "adsb",
          "probe_station",
          "3C6444",
          { kind: "probe_station", data: {} },
          {
            summary: "climbing",
            label: "DLH123",
            fields: [speed("451 kt")],
          },
        ),
      ],
      records: [
        sinkRecord(
          "pager",
          { kind: "probe_plain", data: {} },
          { summary: "second" },
          "2026-09-21T10:00:02Z",
        ),
        sinkRecord(
          "pager",
          { kind: "probe_plain", data: {} },
          { summary: "first" },
          "2026-09-21T10:00:01Z",
        ),
      ],
    });
    const text = readoutText(groups, KINDS, named);
    expect(text).toContain("ADSB · Station\nDLH123  climbing  Speed 451 kt");
    expect(text.indexOf("first")).toBeLessThan(text.indexOf("second"));
    expect(text).toContain("PAGER · Plain");
  });

  it("is empty when nothing arrived", () => {
    expect(readoutText([], KINDS, named)).toBe("");
  });
});

describe("source rows", () => {
  it("counts what each source shows and when it last spoke", () => {
    const groups = groupsOf({
      ...EMPTY_SINK,
      records: [
        sinkRecord(
          "pager",
          { kind: "probe_plain", data: {} },
          { summary: "b" },
          "2026-09-21T10:00:04Z",
        ),
        sinkRecord(
          "pager",
          { kind: "probe_plain", data: {} },
          { summary: "a" },
          "2026-09-21T10:00:01Z",
        ),
      ],
    });
    expect(sourceRows(groups, KINDS, named)).toEqual([
      {
        key: "pager-probe_plain",
        label: "PAGER · Plain",
        count: 2,
        lastMs: Date.parse("2026-09-21T10:00:04Z"),
      },
    ]);
  });
});
