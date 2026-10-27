import { describe, expect, it } from "vitest";
import type { PreRollStatus } from "../lib/types";
import { heldSeconds, preRollFill, preRollPhase } from "./preRoll";

const held: PreRollStatus = {
  node: "rec",
  stream: 0,
  pre_roll_s: 10,
  sample_rate: 2_048_000,
  center_hz: 100_000_000,
  held_samples: 10_240_000,
  capacity_samples: 20_480_000,
  overruns: 0,
};

const capturing: PreRollStatus = {
  ...held,
  capture: {
    file: "rec_1_20260816T000000Z",
    stream: 0,
    started_at: "2026-08-16T00:00:00Z",
    samples: 10_240_000,
    bytes: 81_920_000,
    overruns: 0,
  },
};

describe("preRollPhase", () => {
  it("separates an idle radio from a held window and a running capture", () => {
    expect(preRollPhase({ status: "running" }, "rec")).toEqual({ kind: "idle" });
    expect(preRollPhase({ status: "running", pre_roll: held }, "rec")).toEqual({
      kind: "held",
      status: held,
    });
    expect(preRollPhase({ status: "running", pre_roll: capturing }, "rec")).toEqual({
      kind: "capturing",
      status: capturing,
    });
  });

  it("names the recorder that already holds the radio's pre-roll", () => {
    expect(preRollPhase({ status: "running", pre_roll: held }, "other")).toEqual({
      kind: "busy",
      owner: "rec",
    });
  });

  it("offers nothing while the radio is not running", () => {
    expect(preRollPhase({ status: "error", pre_roll: held }, "rec")).toEqual({
      kind: "unavailable",
    });
    expect(preRollPhase(null, "rec")).toEqual({ kind: "unavailable" });
  });
});

describe("the held window", () => {
  it("reads as seconds and as a fraction of what was asked for", () => {
    expect(heldSeconds(held)).toBe(5);
    expect(preRollFill(held)).toBe(0.5);
    expect(preRollFill({ ...held, held_samples: 40_960_000 })).toBe(1);
    expect(preRollFill({ ...held, capacity_samples: 0 })).toBe(0);
    expect(heldSeconds({ ...held, sample_rate: 0 })).toBe(0);
  });
});
