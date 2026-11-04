import { describe, expect, it } from "vitest";
import { sameMode } from "./useTuner";

describe("sameMode", () => {
  it("treats a missing mode as nothing to say", () => {
    expect(sameMode(null, "voice")).toBe(true);
    expect(sameMode("", "voice")).toBe(true);
  });

  it("matches a decoder ignoring case", () => {
    expect(sameMode("Voice", "voice")).toBe(true);
    expect(sameMode("talk", "voice")).toBe(false);
  });

  it("never matches a device, which runs no mode", () => {
    expect(sameMode("voice", null)).toBe(false);
  });
});
