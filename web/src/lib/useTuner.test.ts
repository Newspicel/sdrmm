import { describe, expect, it } from "vitest";
import { sameMode } from "./useTuner";

describe("sameMode", () => {
  it("treats a missing mode as nothing to say", () => {
    expect(sameMode(null, "nfm")).toBe(true);
    expect(sameMode("", "nfm")).toBe(true);
  });

  it("matches a decoder ignoring case", () => {
    expect(sameMode("NFM", "nfm")).toBe(true);
    expect(sameMode("am", "nfm")).toBe(false);
  });

  it("never matches a device, which runs no mode", () => {
    expect(sameMode("nfm", null)).toBe(false);
  });
});
