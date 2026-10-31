import { describe, expect, it } from "vitest";
import { clockText, daysOn, newWindow, parsedClock, stateLine, withDays } from "./schedule";

describe("clock text", () => {
  it("round trips minutes past midnight", () => {
    expect(clockText(0)).toBe("00:00");
    expect(clockText(23 * 60 + 59)).toBe("23:59");
    expect(parsedClock("07:05")).toBe(7 * 60 + 5);
  });

  it("refuses a time past the day", () => {
    expect(parsedClock("24:00")).toBeNull();
    expect(parsedClock("12:60")).toBeNull();
    expect(parsedClock("")).toBeNull();
  });
});

describe("days", () => {
  it("shows every day for none and saves every day as none", () => {
    const window = newWindow();
    expect(daysOn(window)).toHaveLength(7);
    expect(withDays(window, daysOn(window)).days).toEqual([]);
  });

  it("keeps picked days in week order", () => {
    expect(withDays(newWindow(), ["sunday", "monday"]).days).toEqual(["monday", "sunday"]);
  });
});

describe("stateLine", () => {
  const at = Date.UTC(2026, 9, 28, 8, 30);
  const status = { node: "s", on: true, zone: "UTC" };

  it("says when it next flips, on the schedule's clock", () => {
    const next = Date.UTC(2026, 9, 28, 9, 0) / 1_000;
    expect(stateLine({ ...status, next_change: next }, 1, at)).toBe("on until 09:00");
  });

  it("names the day of a change more than a day away", () => {
    const next = Date.UTC(2026, 9, 30, 6, 0) / 1_000;
    expect(stateLine({ ...status, on: false, next_change: next }, 1, at)).toMatch(
      /^off until \S+ 06:00$/,
    );
  });

  it("says when it has no windows and stays quiet on a fault", () => {
    expect(stateLine(null, 0, at)).toBe("no windows");
    expect(stateLine({ ...status, error: "Unknown time zone" }, 1, at)).toBeUndefined();
  });
});
