import { describe, expect, it } from "vitest";
import type { PatchNode } from "../../lib/types";
import { outputMissing, speakerOutputOf, withSpeakerOutput } from "./speakerOutput";

const speaker: PatchNode = { id: "s", kind: "speaker", data: null, position: { x: 0, y: 0 } };

describe("speaker output", () => {
  it("keeps the picked output by name and clears it back to System", () => {
    const picked = withSpeakerOutput(speaker, "js8");
    expect(speakerOutputOf(picked)).toBe("js8");
    expect(speakerOutputOf(withSpeakerOutput(picked, null))).toBeNull();
  });

  it("flags a saved output only once the names are known and it is gone", () => {
    const listed = { outputs: [{ id: "a", name: "js8" }], named: true };
    expect(outputMissing("js8", listed)).toBe(false);
    expect(outputMissing("fldigi", listed)).toBe(true);
    expect(outputMissing("fldigi", { outputs: [], named: false })).toBe(false);
    expect(outputMissing("fldigi", null)).toBe(false);
    expect(outputMissing(null, listed)).toBe(false);
  });
});
