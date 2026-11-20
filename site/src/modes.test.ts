import { describe, expect, it } from "vitest";
import {
  ALL,
  allocations,
  band,
  bars,
  CATALOG,
  type Catalog,
  counts,
  familyTitle,
  levelOf,
  type Mode,
  matches,
  position,
  rate,
  shown,
} from "./modes";

function mode(name: string, family: string): Mode {
  return {
    type_id: name.toLowerCase(),
    name,
    summary: "",
    family,
    verification: "on_air",
    input_rate_hz: 48_000,
    bands: [],
  };
}

const sample: Catalog = {
  levels: [{ id: "on_air", label: "Tested on air", means: "" }],
  families: [
    { id: "digital_voice", title: "Digital voice" },
    { id: "aviation", title: "Aviation" },
  ],
  modes: [mode("DMR", "digital_voice"), mode("D-STAR", "digital_voice"), mode("ACARS", "aviation")],
};

describe("search", () => {
  it("finds a mode by any part of its name, ignoring case", () => {
    const dmr = mode("DMR", "digital_voice");
    expect(matches(dmr, "dm")).toBe(true);
    expect(matches(dmr, "  DMR ")).toBe(true);
    expect(matches(dmr, "acars")).toBe(false);
    expect(matches(dmr, "")).toBe(true);
  });

  it("narrows to the picked levels", () => {
    const acars = mode("ACARS", "aviation");
    expect(shown(acars, "", new Set())).toBe(true);
    expect(shown(acars, "", new Set(["on_air"]))).toBe(true);
    expect(shown(acars, "", new Set(["own_modulator"]))).toBe(false);
    expect(shown(acars, "dmr", new Set())).toBe(false);
  });

  it("counts what the search leaves in every group", () => {
    expect(counts(sample, "")).toEqual(
      new Map([
        [ALL, 3],
        ["digital_voice", 2],
        ["aviation", 1],
      ]),
    );
    expect(counts(sample, "d-").get("digital_voice")).toBe(1);
    expect(counts(sample, "d-").get("aviation")).toBe(0);
    expect(counts(sample, "", new Set(["experimental"])).get(ALL)).toBe(0);
  });
});

describe("formatting", () => {
  it("reads a rate in samples per second", () => {
    expect(rate(48_000)).toBe("48 kS/s");
    expect(rate(2_048_000)).toBe("2.048 MS/s");
    expect(rate(9_142_857.142857144)).toBe("9.1429 MS/s");
  });

  it("spans every allocation in the unit of the top edge", () => {
    expect(band([])).toBe("");
    expect(
      band([
        { low: 144e6, high: 148e6 },
        { low: 430e6, high: 450e6 },
      ]),
    ).toBe("144 to 450 MHz");
    expect(band([{ low: 39_500, high: 162_500 }])).toBe("39.5 to 162.5 kHz");
    expect(allocations([{ low: 1089e6, high: 1091e6 }])).toBe("1.089 GHz to 1.091 GHz");
  });
});

describe("the spectrum strip", () => {
  it("places a frequency on a log scale from LF to UHF", () => {
    expect(position(30e3)).toBe(0);
    expect(position(3e9)).toBe(1);
    expect(position(3e6)).toBeCloseTo(0.4);
    expect(position(1)).toBe(0);
  });

  it("stacks overlapping allocations into separate lanes", () => {
    const wide = { ...mode("Wide", "aviation"), bands: [{ low: 100e6, high: 200e6 }] };
    const inside = { ...mode("Inside", "aviation"), bands: [{ low: 120e6, high: 121e6 }] };
    const apart = { ...mode("Apart", "aviation"), bands: [{ low: 1e9, high: 1.1e9 }] };
    const lanes = new Map(bars([wide, inside, apart]).map((bar) => [bar.mode.name, bar.lane]));
    expect(lanes).toEqual(
      new Map([
        ["Wide", 0],
        ["Inside", 1],
        ["Apart", 0],
      ]),
    );
  });

  it("draws every allocation of the catalog", () => {
    const allocations = CATALOG.modes.reduce((sum, entry) => sum + entry.bands.length, 0);
    expect(bars(CATALOG.modes)).toHaveLength(allocations);
  });
});

describe("the generated catalog", () => {
  it("names a group and a level for every mode", () => {
    expect(CATALOG.modes.length).toBeGreaterThan(0);
    for (const entry of CATALOG.modes) {
      expect(familyTitle(CATALOG, entry.family)).not.toBe(entry.family);
      expect(levelOf(CATALOG, entry.verification), entry.type_id).toBeDefined();
    }
  });

  it("counts every mode once", () => {
    const tally = counts(CATALOG, "");
    const grouped = CATALOG.families.reduce((sum, family) => sum + (tally.get(family.id) ?? 0), 0);
    expect(grouped).toBe(CATALOG.modes.length);
  });
});
