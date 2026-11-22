import { describe, expect, it } from "vitest";
import type { PatchNode } from "../../lib/types";
import { choicesOf } from "./SpeakerOutputChip";
import {
  choiceOf,
  listenerOutputOf,
  outputMissing,
  outputOfChoice,
  serverOutputOf,
  speakerOutputOf,
  withSpeakerOutput,
} from "./speakerOutput";

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

  it("tells the server apart from this machine", () => {
    expect(serverOutputOf({ server: null })).toBeNull();
    expect(serverOutputOf({ server: "USB" })).toBe("USB");
    expect(serverOutputOf("USB")).toBeUndefined();
    expect(serverOutputOf(null)).toBeUndefined();
    expect(listenerOutputOf({ server: "USB" })).toBeNull();
    expect(listenerOutputOf("USB")).toBe("USB");
  });

  it("round-trips every output through its choice", () => {
    for (const output of [null, "USB", { server: null }, { server: "USB" }]) {
      expect(outputOfChoice(choiceOf(output))).toEqual(output);
    }
    expect(choiceOf("USB")).not.toBe(choiceOf({ server: "USB" }));
  });

  it("offers server cards after this machine's", () => {
    const labels = choicesOf(["Headphones"], ["USB"], null, false).map((choice) => choice.label);
    expect(labels).toEqual(["System", "Headphones", "Server", "Server: USB"]);
  });

  it("offers no server when it cannot list its cards", () => {
    const labels = choicesOf([], null, null, false).map((choice) => choice.label);
    expect(labels).toEqual(["System"]);
  });

  it("keeps a saved server card that went away", () => {
    const choices = choicesOf([], [], { server: "USB" }, true);
    expect(choices.at(-1)).toEqual({
      value: choiceOf({ server: "USB" }),
      label: "Server: USB (not connected)",
    });
  });
});
