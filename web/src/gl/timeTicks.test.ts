import { describe, expect, it } from "vitest";
import { formatClock, timeTicks } from "./timeTicks";

const NOW = 1_700_000_000_000;

function steady(back: number): number {
  return NOW - back * 100;
}

describe("timeTicks", () => {
  it("marks the rows where the clock passes a step", () => {
    const ticks = timeTicks(steady, 101, 100, 20);
    expect(ticks.map((tick) => tick.y)).toEqual([0, 20, 40, 60, 80]);
    expect(ticks[1]?.label).toBe(formatClock(NOW - 2_000, true));
  });

  it("keeps a mark on its row as newer rows push it down", () => {
    const before = timeTicks(steady, 101, 100, 20);
    const after = timeTicks((back) => steady(back - 7), 101, 100, 20);
    expect(after[1]).toEqual({ y: (before[1]?.y ?? 0) + 7, label: before[1]?.label });
  });

  it("drops the seconds once marks are minutes apart", () => {
    const ticks = timeTicks((back) => NOW - back * 10_000, 61, 600, 40);
    expect(ticks[0]?.label).toBe(formatClock(NOW - 20_000, false));
  });

  it("draws nothing without timestamps", () => {
    expect(timeTicks(() => Number.NaN, 100, 100, 20)).toEqual([]);
    expect(timeTicks((back) => (back === 0 ? NOW : Number.NaN), 100, 100, 20)).toEqual([]);
  });
});
