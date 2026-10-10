import { describe, expect, it } from "vitest";
import { isLastOn, keptOn } from "./Segmented";

describe("keptOn", () => {
  const options = [
    { value: 0, label: "RX1" },
    { value: 1, label: "RX2" },
    { value: 2, label: "RX3", disabled: true },
  ];

  it("keeps the pressed options in their own order", () => {
    expect(keptOn(options, ["1", "0"])).toEqual([0, 1]);
    expect(keptOn(options, ["1"])).toEqual([1]);
  });

  it("drops disabled and unknown values", () => {
    expect(keptOn(options, ["2", "7"])).toEqual([]);
  });
});

describe("isLastOn", () => {
  it("locks only the one toggle left on", () => {
    expect(isLastOn([1], 1)).toBe(true);
    expect(isLastOn([1], 0)).toBe(false);
    expect(isLastOn([0, 1], 0)).toBe(false);
  });
});
