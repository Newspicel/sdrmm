import { describe, expect, it } from "vitest";
import type { DecoderEvent } from "../lib/types";
import { CATALOG } from "../test/catalog";
import { sinkEntity } from "../test/sinks";
import { legendGroups } from "./MapPanel";

const PLANE: DecoderEvent = { kind: "adsb", data: { icao: "3c6444", df: 17, raw: "8d" } };
const AT = { place: { at: { lat: 52.5, lon: 13.4 } } };

describe("map legend groups", () => {
  it("lists two ADS-B decoders as two groups with their own counts", () => {
    const entities = [
      sinkEntity("north", "adsb", "A", PLANE, { summary: "A", ...AT }),
      sinkEntity("north", "adsb", "B", PLANE, { summary: "B", ...AT }),
      sinkEntity("south", "adsb", "A", PLANE, { summary: "A", ...AT }),
      sinkEntity("south", "adsb", "C", PLANE, { summary: "C" }),
    ];
    const names: Record<string, string> = { north: "North", south: "South" };

    const groups = legendGroups(
      { entities, tracks: [], sourceName: (source) => names[source] ?? source },
      CATALOG.event_kinds,
      () => "#21b0b0",
    );

    expect(groups.map((group) => [group.label, group.count])).toEqual([
      ["North · Aircraft", 2],
      ["South · Aircraft", 1],
    ]);
  });
});
