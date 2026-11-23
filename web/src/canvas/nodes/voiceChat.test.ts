import { describe, expect, it } from "vitest";
import type { PatchNode } from "../../lib/types";
import {
  allowedEntries,
  allowedOf,
  KEYLESS_SEND_MODES,
  withGate,
  withVoiceChat,
} from "./voiceChat";

const GATE = { mode: "voice_activity", threshold_db: -35, hang_ms: 600, volume: 1 } as const;

const chat = {
  id: "vc",
  kind: "voice_chat",
  position: { x: 0, y: 0 },
  data: {
    account: "a1",
    channel: "123",
    allowed: ["42"],
    to_air: GATE,
    to_chat: { ...GATE, mode: "continuous" },
  },
} as PatchNode;

describe("withGate", () => {
  it("changes one field of one direction and keeps every other field", () => {
    expect(withGate(chat, "to_air", { threshold_db: -50 })).toEqual({
      ...chat,
      data: {
        account: "a1",
        channel: "123",
        allowed: ["42"],
        to_air: { mode: "voice_activity", threshold_db: -50, hang_ms: 600, volume: 1 },
        to_chat: { mode: "continuous", threshold_db: -35, hang_ms: 600, volume: 1 },
      },
    });
    expect(withGate(chat, "to_chat", { mode: "voice_activity", hang_ms: 1_200 })).toMatchObject({
      data: {
        to_air: GATE,
        to_chat: { mode: "voice_activity", threshold_db: -35, hang_ms: 1_200, volume: 1 },
      },
    });
  });

  it("edits volume and keeps the other gate fields", () => {
    expect(withGate(chat, "to_air", { volume: 0.5 })).toMatchObject({
      data: { to_air: { ...GATE, volume: 0.5 }, to_chat: { mode: "continuous", volume: 1 } },
    });
  });

  it("leaves any other node alone", () => {
    const other = { id: "s", kind: "scope", position: { x: 0, y: 0 } } as PatchNode;
    expect(withGate(other, "to_air", { hang_ms: 0 })).toBe(other);
    expect(withVoiceChat(other, { channel: "x" })).toBe(other);
  });
});

describe("withVoiceChat", () => {
  it("keeps the gates when a setting changes", () => {
    expect(withVoiceChat(chat, { account: "a2" })).toMatchObject({
      data: { account: "a2", channel: "123", to_air: GATE },
    });
  });
});

describe("allowed", () => {
  it("round-trips ids and drops blanks", () => {
    expect(allowedOf(allowedEntries(["1", "2"]))).toEqual(["1", "2"]);
    expect(allowedOf([{ id: " 3 " }, { id: "" }, {}])).toEqual(["3"]);
  });
});

describe("send modes", () => {
  it("offer a voice chat no push to talk", () => {
    expect(KEYLESS_SEND_MODES.map((mode) => mode.value)).toEqual(["voice_activity", "continuous"]);
  });
});
