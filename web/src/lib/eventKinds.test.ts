import { describe, expect, it } from "vitest";
import { CATALOG } from "../test/catalog";
import { isEventKind, kindHas, kindLabel } from "./eventKinds";

const KINDS = CATALOG.event_kinds;

describe("event kinds", () => {
  it("labels every kind the server serves", () => {
    expect(KINDS.length).toBeGreaterThan(0);
    for (const entry of KINDS) {
      expect(kindLabel(entry.kind, KINDS)).toBe(entry.name);
    }
    expect(kindLabel("adsb", KINDS)).toBe("ADS-B");
    expect(kindLabel("ident", KINDS)).toBe("Signal ID");
  });

  it("falls back for a kind this build does not know", () => {
    expect(kindLabel("dmr", KINDS)).toBe("DMR");
    expect(isEventKind("dmr", KINDS)).toBe(false);
    expect(isEventKind("navtex", KINDS)).toBe(true);
  });

  it("reads capabilities from the row", () => {
    expect(kindHas("adsb", "geo", KINDS)).toBe(true);
    expect(kindHas("call", "voice", KINDS)).toBe(true);
    expect(kindHas("pocsag", "geo", KINDS)).toBe(false);
    expect(kindHas("dmr", "log", KINDS)).toBe(false);
  });
});
