import { beforeEach, describe, expect, it } from "vitest";
import { DEFAULT_AVERAGE } from "../../components/videoAverage";
import { readAverage } from "./scopeDisplay";

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
