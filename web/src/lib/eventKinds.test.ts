import { describe, expect, it } from "vitest";
import { PROBE_CATALOG } from "../test/kinds";
import { isEventKind, kindHas, kindLabel } from "./eventKinds";

const KINDS = PROBE_CATALOG.event_kinds;

describe("event kinds", () => {
  it("labels every kind the server serves", () => {
    expect(KINDS.length).toBeGreaterThan(0);
    for (const entry of KINDS) {
      expect(kindLabel(entry.kind, KINDS)).toBe(entry.name);
    }
    expect(kindLabel("probe_plane", KINDS)).toBe("Probe plane");
    expect(kindLabel("ident", KINDS)).toBe("Signal ID");
  });

  it("falls back for a kind this build does not know", () => {
    expect(kindLabel("unheard", KINDS)).toBe("UNHEARD");
    expect(isEventKind("unheard", KINDS)).toBe(false);
    expect(isEventKind("probe_text", KINDS)).toBe(true);
  });

  it("reads capabilities from the row", () => {
    expect(kindHas("probe_plane", "geo", KINDS)).toBe(true);
    expect(kindHas("call", "voice", KINDS)).toBe(true);
    expect(kindHas("probe_pager", "geo", KINDS)).toBe(false);
    expect(kindHas("unheard", "log", KINDS)).toBe(false);
  });
});
