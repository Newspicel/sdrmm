import { describe, expect, it } from "vitest";
import type { DecoderEvent, EventKindEntry } from "../lib/types";
import { CATALOG } from "../test/catalog";
import { sinkEntity } from "../test/sinks";
import { legendGroups } from "./MapPanel";

const PLANE: DecoderEvent = { kind: "probe", data: {} };
const PROBES: EventKindEntry = {
  kind: "probe",
  name: "Probe",
  noun: "Probes",
  caps: ["station", "geo"],
  hue: "teal",
  lifetime: { type: "instant" },
};
const AT = { place: { at: { lat: 52.5, lon: 13.4 } } };

describe("map legend groups", () => {
  it("lists two decoders of one kind as two groups with their own counts", () => {
    const entities = [
      sinkEntity("north", "probe", "A", PLANE, { summary: "A", ...AT }),
      sinkEntity("north", "probe", "B", PLANE, { summary: "B", ...AT }),
      sinkEntity("south", "probe", "A", PLANE, { summary: "A", ...AT }),
      sinkEntity("south", "probe", "C", PLANE, { summary: "C" }),
    ];
    const names: Record<string, string> = { north: "North", south: "South" };

    const groups = legendGroups(
      { entities, tracks: [], sourceName: (source) => names[source] ?? source },
      [...CATALOG.event_kinds, PROBES],
      () => "#21b0b0",
    );

    expect(groups.map((group) => [group.label, group.count])).toEqual([
      ["North · Probes", 2],
      ["South · Probes", 1],
    ]);
  });
});
