import { describe, expect, it, vi } from "vitest";
import type { NodeActed, NodeAction, TimeMachineStatus } from "../lib/types";
import {
  heldSeconds,
  historyFill,
  timeMachineMutationOptions,
  timeMachinePhase,
} from "./timeMachine";

const held: TimeMachineStatus = {
  node: "history",
  stream: 0,
  history_seconds: 10,
  sample_rate: 2_048_000,
  center_hz: 100_000_000,
  held_samples: 10_240_000,
  capacity_samples: 20_480_000,
  overruns: 0,
};

const capturing: TimeMachineStatus = {
  ...held,
  capture: {
    file: "tm_1_20260816T000000Z",
    stream: 0,
    started_at: "2026-08-16T00:00:00Z",
    samples: 10_240_000,
    bytes: 81_920_000,
    overruns: 0,
  },
};

describe("timeMachinePhase", () => {
  it("separates an idle radio from an armed buffer and a running capture", () => {
    expect(timeMachinePhase({ status: "running" }, "history")).toEqual({ kind: "idle" });
    expect(timeMachinePhase({ status: "running", time_machine: held }, "history")).toEqual({
      kind: "armed",
      status: held,
    });
    expect(timeMachinePhase({ status: "running", time_machine: capturing }, "history")).toEqual({
      kind: "capturing",
      status: capturing,
    });
  });

  it("names the node that already holds the radio's history", () => {
    expect(timeMachinePhase({ status: "running", time_machine: held }, "other")).toEqual({
      kind: "busy",
      owner: "history",
    });
  });

  it("offers nothing while the radio is not running", () => {
    expect(timeMachinePhase({ status: "error", time_machine: held }, "history")).toEqual({
      kind: "unavailable",
    });
    expect(timeMachinePhase(null, "history")).toEqual({ kind: "unavailable" });
  });
});

describe("the held window", () => {
  it("reads as seconds and as a fraction of what was asked for", () => {
    expect(heldSeconds(held)).toBe(5);
    expect(historyFill(held)).toBe(0.5);
    expect(historyFill({ ...held, held_samples: 40_960_000 })).toBe(1);
    expect(historyFill({ ...held, capacity_samples: 0 })).toBe(0);
    expect(heldSeconds({ ...held, sample_rate: 0 })).toBe(0);
  });
});

describe("timeMachineMutationOptions", () => {
  it("sends each action to the node, which the server resolves to its radio", async () => {
    const seen: unknown[] = [];
    const request = vi.fn(async (_node: string, action: NodeAction): Promise<NodeActed> => {
      seen.push(action);
      return { result: "time_machine", with: held };
    });
    const options = timeMachineMutationOptions("history", request);

    await options.mutationFn("capture");
    await options.mutationFn("stop");

    expect(seen).toEqual([
      { action: "time_machine", with: "capture" },
      { action: "time_machine", with: "stop" },
    ]);
    expect(request).toHaveBeenLastCalledWith("history", {
      action: "time_machine",
      with: "stop",
    });
  });

  it("reports the server's refusal", () => {
    const notify = vi.fn();
    const options = timeMachineMutationOptions("history", vi.fn(), notify);

    options.onError(new Error("Arm it first"));
    expect(notify).toHaveBeenCalledWith("Arm it first");
  });
});
