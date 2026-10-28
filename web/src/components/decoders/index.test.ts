import { describe, expect, it } from "vitest";
import catalog from "../../generated/catalog-fixture.json" with { type: "json" };
import { DECODER_PANELS, MODE_CHIPS } from "./index";

const KINDS = new Set((catalog.event_kinds as { kind: string }[]).map((entry) => entry.kind));

describe("the decoder registries", () => {
  it("answers for every channel type, naming the ones that carry no chips", () => {
    const silent = Object.entries(MODE_CHIPS)
      .filter(([, chips]) => chips === null)
      .map(([type]) => type);

    expect(silent).toEqual([
      "dstar",
      "ysf",
      "p25",
      "dpmr",
      "m17",
      "tetra",
      "dsc",
      "inmarsat_stdc",
      "vdl2",
      "hfdl",
    ]);
    for (const chips of Object.values(MODE_CHIPS)) {
      expect(chips === null || typeof chips === "function").toBe(true);
    }
  });

  it("keys every panel to a decoder kind the wire publishes", () => {
    const keys = Object.keys(DECODER_PANELS);
    expect(keys.length).toBeGreaterThan(20);
    expect(keys.filter((kind) => !KINDS.has(kind))).toEqual([]);
    for (const panel of Object.values(DECODER_PANELS)) {
      expect(typeof panel).toBe("function");
    }
  });
});
