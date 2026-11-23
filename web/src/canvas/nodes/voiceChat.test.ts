import { describe, expect, it } from "vitest";
import type { Account, PatchNode } from "../../lib/types";
import {
  allowedEntries,
  allowedOf,
  channelTitle,
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
    from_channel: GATE,
    to_channel: { ...GATE, mode: "continuous" },
  },
} as PatchNode;

describe("channelTitle", () => {
  it("channel title names rooms for matrix", () => {
    const account = { id: "m", name: "M", service: "matrix", homeserver_url: "h" } as Account;
    expect(channelTitle(account)).toBe("Room id or alias");
  });
});

describe("withGate", () => {
  it("changes one field of one direction and keeps every other field", () => {
    expect(withGate(chat, "from_channel", { threshold_db: -50 })).toEqual({
      ...chat,
      data: {
        account: "a1",
        channel: "123",
        allowed: ["42"],
        from_channel: { mode: "voice_activity", threshold_db: -50, hang_ms: 600, volume: 1 },
        to_channel: { mode: "continuous", threshold_db: -35, hang_ms: 600, volume: 1 },
      },
    });
    expect(withGate(chat, "to_channel", { mode: "voice_activity", hang_ms: 1_200 })).toMatchObject({
      data: {
        from_channel: GATE,
        to_channel: { mode: "voice_activity", threshold_db: -35, hang_ms: 1_200, volume: 1 },
      },
    });
  });

  it("edits volume and keeps the other gate fields", () => {
    expect(withGate(chat, "from_channel", { volume: 0.5 })).toMatchObject({
      data: {
        from_channel: { ...GATE, volume: 0.5 },
        to_channel: { mode: "continuous", volume: 1 },
      },
    });
  });

  it("leaves any other node alone", () => {
    const other = { id: "s", kind: "scope", position: { x: 0, y: 0 } } as PatchNode;
    expect(withGate(other, "from_channel", { hang_ms: 0 })).toBe(other);
    expect(withVoiceChat(other, { channel: "x" })).toBe(other);
  });
});

describe("withVoiceChat", () => {
  it("keeps the gates when a setting changes", () => {
    expect(withVoiceChat(chat, { account: "a2" })).toMatchObject({
      data: { account: "a2", channel: "123", from_channel: GATE },
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
