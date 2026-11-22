import { describe, expect, it } from "vitest";
import { captures } from "./useMicrophone";

describe("captures", () => {
  it("stays closed without an armed transmitter in every mode", () => {
    for (const send of ["push_to_talk", "voice_activity", "continuous"] as const) {
      expect(captures({ send, armed: false, held: true }, true)).toBe(false);
    }
  });

  it("opens push to talk on the key and the other modes without it", () => {
    expect(captures({ send: "push_to_talk", armed: true, held: false }, false)).toBe(false);
    expect(captures({ send: "push_to_talk", armed: true, held: false }, true)).toBe(true);
    expect(captures({ send: "voice_activity", armed: true, held: false }, false)).toBe(true);
    expect(captures({ send: "continuous", armed: true, held: false }, false)).toBe(true);
  });
});
