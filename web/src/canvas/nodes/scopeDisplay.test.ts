import { beforeEach, describe, expect, it } from "vitest";
import { DEFAULT_AVERAGE } from "../../components/videoAverage";
import { isDbWindow, isSpectrumView, readAverage, readOverlap } from "./scopeDisplay";

const AVERAGE_KEY = "sdrmm.scopeAverage";

describe("readAverage", () => {
  beforeEach(() => localStorage.clear());

  it("takes a remembered choice the video averager offers", () => {
    localStorage.setItem(AVERAGE_KEY, "8");
    expect(readAverage()).toBe(8);
  });

  it("falls back for a stored value no longer offered", () => {
    localStorage.setItem(AVERAGE_KEY, "7");
    expect(readAverage()).toBe(DEFAULT_AVERAGE);
    localStorage.clear();
    expect(readAverage()).toBe(DEFAULT_AVERAGE);
  });
});

describe("readOverlap", () => {
  beforeEach(() => localStorage.clear());

  it("takes a remembered overlap and falls back to auto", () => {
    localStorage.setItem("sdrmm.scopeOverlap", "full");
    expect(readOverlap()).toBe("full");
    localStorage.setItem("sdrmm.scopeOverlap", "most");
    expect(readOverlap()).toBe("auto");
  });
});

describe("remembered scope views", () => {
  it("restore only a zoom inside the full span", () => {
    expect(isSpectrumView({ start: 0.2, end: 0.6 })).toBe(true);
    expect(isSpectrumView({ start: 0.6, end: 0.2 })).toBe(false);
    expect(isSpectrumView({ start: -1, end: 0.5 })).toBe(false);
    expect(isSpectrumView(null)).toBe(false);
  });

  it("restore only an ordered dB range", () => {
    expect(isDbWindow({ min: -120, max: -20 })).toBe(true);
    expect(isDbWindow({ min: -20, max: -120 })).toBe(false);
    expect(isDbWindow({ min: "x", max: 0 })).toBe(false);
  });
});
