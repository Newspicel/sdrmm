import { describe, expect, it } from "vitest";
import type { PatchGraph, ProcessorStatus } from "../../lib/types";
import { CATALOG, catalogBody } from "../../test/catalog";
import { arrayStatus, laneStatus, placed } from "../../test/fixtures";
import { resolvedBy } from "../../test/recorded";
import { nodeOf } from "../graph";
import {
  ageLabel,
  faceChips,
  NO_ARRAY,
  processorFaults,
  processorGate,
  processorKindOf,
  processorSubtitle,
  STALE,
  tuneArrayEdit,
  tuningAsk,
} from "./processorFace";

const WIRED: PatchGraph = {
  nodes: [placed("arr", catalogBody("array")), placed("df", catalogBody("df"))],
  edges: [{ from: { node: "arr", port: "array" }, to: { node: "df", port: "array" } }],
};

const UNWIRED: PatchGraph = { nodes: WIRED.nodes, edges: [] };

function gated(processor: string, gate: ProcessorStatus["gated"]): ProcessorStatus {
  return {
    node: processor,
    kind: "df",
    running: true,
    gated: gate,
    gated_samples: 0,
    dropped_samples: 0,
    dropped_reports: 0,
    lane_overflows: 0,
    lane_mismatch: 0,
    solver_failures: 0,
    resets: 0,
  };
}

const FIVE = arrayStatus("arr", { lanes: [0, 1, 2, 3, 4].map((lane) => laneStatus(lane)) });
const FRESH = { view: {}, receivedAt: 10_000, periodMs: 500 };

function text(...args: Parameters<typeof processorSubtitle>): string {
  return processorSubtitle(...args).text;
}

describe("processorSubtitle", () => {
  it("says no array, the gate, stale or the lane count", () => {
    expect(text(resolvedBy(UNWIRED), "df", FIVE, FRESH, 10_000)).toBe(NO_ARRAY);
    const calibrating = { ...FIVE, processors: [gated("df", "calibrating")] };
    expect(text(resolvedBy(WIRED), "df", calibrating, FRESH, 10_000)).toBe("calibrating");
    const phase = { ...FIVE, processors: [gated("df", "phase")] };
    expect(text(resolvedBy(WIRED), "df", phase, FRESH, 10_000)).toBe("needs cal");
    expect(text(resolvedBy(WIRED), "df", FIVE, FRESH, 12_001)).toBe(STALE);
    expect(text(resolvedBy(WIRED), "df", FIVE, FRESH, 11_000)).toBe("5 lanes");
    expect(text(resolvedBy(WIRED), "df", FIVE, undefined, 99_000)).toBe("5 lanes");
  });

  it("puts a warning headline before staleness and a plain one after", () => {
    const warned = { ...FRESH, view: { headline: { text: "No transmitter", warn: true } } };
    expect(processorSubtitle(resolvedBy(WIRED), "df", FIVE, warned, 99_000)).toEqual({
      text: "No transmitter",
      warn: true,
    });
    const tuned = { ...FRESH, view: { headline: { text: "98.10 MHz", warn: false } } };
    expect(text(resolvedBy(WIRED), "df", FIVE, tuned, 12_001)).toBe(STALE);
    expect(text(resolvedBy(WIRED), "df", FIVE, tuned, 10_500)).toBe("98.10 MHz");
  });

  it("counts wired lanes before the array reports", () => {
    const lanes: PatchGraph = {
      nodes: [...WIRED.nodes, placed("radio", { kind: "device", data: {} })],
      edges: [
        ...(WIRED.edges ?? []),
        { from: { node: "radio", port: "iq" }, to: { node: "arr", port: "lane" } },
        { from: { node: "radio", port: "iq2" }, to: { node: "arr", port: "lane2" } },
      ],
    };
    expect(text(resolvedBy(lanes), "df", undefined, undefined, 0)).toBe("2 lanes");
  });

  it("reads the gate from the array's processor list only", () => {
    const other = { ...FIVE, processors: [gated("beam", "sync")] };
    expect(processorGate(other, "df")).toBeNull();
    expect(processorGate(other, "beam")).toBe("sync");
  });
});

describe("processorFaults", () => {
  it("lists only the counters that moved", () => {
    expect(processorFaults(null)).toEqual([]);
    expect(processorFaults(gated("df", undefined))).toEqual([]);
    const faults = processorFaults({
      ...gated("df", undefined),
      dropped_samples: 4,
      lane_overflows: 1,
      truncated: 2,
      solver_failures: 5,
    });
    expect(faults.map((row) => [row.label, row.count])).toEqual([
      ["Drops", 4],
      ["Cut", 3],
      ["Fails", 5],
    ]);
  });
});

describe("ageLabel", () => {
  it("formats ages", () => {
    expect(ageLabel(undefined, 1_000)).toBe("-");
    expect(ageLabel(1_000, 1_400)).toBe("0.4 s");
    expect(ageLabel(0, 12_300)).toBe("12 s");
    expect(ageLabel(0, 185_000)).toBe("3 min");
  });
});

function stitched(wired: boolean): PatchGraph {
  return {
    nodes: [placed("arr", catalogBody("array")), placed("wide", catalogBody("stitch"))],
    edges: wired
      ? [{ from: { node: "arr", port: "array" }, to: { node: "wide", port: "array" } }]
      : [],
  };
}

describe("array tuning a processor needs", () => {
  it("asks for the tuning the processor declares until the array has it", () => {
    expect(tuningAsk(stitched(true), "arr", "spread")).toBe("spread");
    expect(tuningAsk(stitched(true), "arr", "together")).toBeNull();
    expect(tuningAsk(stitched(true), "arr", null)).toBeNull();
    expect(tuningAsk(stitched(false), null, "spread")).toBeNull();
    const next = tuneArrayEdit(stitched(true), "arr", "spread");
    const array = next === null ? undefined : nodeOf(next, "arr");
    expect(array?.kind === "array" ? array.data.tuning : null).toBe("spread");
    expect(next === null ? "edit" : tuningAsk(next, "arr", "spread")).toBeNull();
    expect(tuneArrayEdit(stitched(false), null, "spread")).toBeNull();
  });

  it("comes from the catalog, not the processor's name", () => {
    const stitch = processorKindOf(CATALOG, placed("wide", catalogBody("stitch")));
    expect(stitch.facets.tuning).toBe("spread");
    expect(stitch.loaded).toBe(true);
  });
});

describe("faceChips", () => {
  it("puts the tuning ask, then staleness, then the reading's flags", () => {
    const state = { ...FRESH, view: { flags: [{ label: "Mirror", title: "x" }] } };
    expect(faceChips(state, true, "spread").map((chip) => chip.label)).toEqual([
      "Needs spread",
      "Stale",
      "Mirror",
    ]);
    expect(faceChips(undefined, false, null)).toEqual([]);
  });
});
