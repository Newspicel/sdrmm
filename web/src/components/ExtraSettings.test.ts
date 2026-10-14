import { expect, it } from "vitest";
import type { ExtraSetting } from "../lib/types";
import { grouped, groupSummary } from "./ExtraSettings";

const toggle = (name: string, group?: string): ExtraSetting => ({
  kind: "bool",
  name,
  default: false,
  group,
});

const level: ExtraSetting = {
  kind: "range",
  name: "agc_high",
  group: "AGC",
  range: { min: -40, max: -1, step: 1 },
  unit: "dBFS",
};

it("gathers a group where its first member stands and keeps loose settings apart", () => {
  const entries = grouped([
    toggle("quadrature", "Tracking"),
    toggle("loopback"),
    toggle("rf_dc", "Tracking"),
    level,
  ]);
  expect(entries.map((entry) => [entry.group, entry.members.map((m) => m.name)])).toEqual([
    ["Tracking", ["quadrature", "rf_dc"]],
    [null, ["loopback"]],
    ["AGC", ["agc_high"]],
  ]);
});

it("sums a group up by its switches, or by its first value when it has none", () => {
  const members = [toggle("tone_1", "Tone"), toggle("tone_2", "Tone")];
  expect(groupSummary(members, () => false)).toBe("off");
  expect(groupSummary(members, (name) => name === "tone_1")).toBe("1/2");
  expect(groupSummary(members, () => true)).toBe("on");
  expect(groupSummary([level], () => -12)).toBe("-12");
});
