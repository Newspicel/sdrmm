import type { Options } from "../../components/controls";
import { VOICE_CHAT_LIMITS } from "../../lib/limits";
import type { Form, Settings } from "../../lib/settingsForm";
import type {
  Account,
  PatchNode,
  PatchNodeOf,
  SendMode,
  VoiceChatStatus,
  VoiceGateSettings,
} from "../../lib/types";

export type VoiceChatSettings = PatchNodeOf<"voice_chat">["data"];
export type GateDirection = "to_air" | "to_chat";

export const SEND_MODES: Options<SendMode> = [
  { value: "push_to_talk", label: "Push to talk", title: "Sends while the key is held" },
  { value: "voice_activity", label: "Voice activity", title: "Sends while someone speaks" },
  { value: "continuous", label: "Continuous", title: "Sends all the time" },
];

/** A server holds no talk key, so a voice chat sends by voice or all the time. */
export const KEYLESS_SEND_MODES: Options<SendMode> = SEND_MODES.filter(
  (mode) => mode.value !== "push_to_talk",
);

export const ALLOWED_ITEM: Form = [
  {
    key: "id",
    label: "User id",
    control: { type: "text", placeholder: "User id", max_len: VOICE_CHAT_LIMITS.allowed_len },
  },
];

export const STATE_TEXT: Record<VoiceChatStatus["state"], string | undefined> = {
  idle: undefined,
  connecting: "connecting",
  joined: "in channel",
};

export function withVoiceChat(node: PatchNode, next: Partial<VoiceChatSettings>): PatchNode {
  return node.kind === "voice_chat" ? { ...node, data: { ...node.data, ...next } } : node;
}

export function withGate(
  node: PatchNode,
  direction: GateDirection,
  next: Partial<VoiceGateSettings>,
): PatchNode {
  return node.kind === "voice_chat"
    ? { ...node, data: { ...node.data, [direction]: { ...node.data[direction], ...next } } }
    : node;
}

export function channelTitle(account: Account | undefined): string {
  if (account?.service === "teamspeak") {
    return "TeamSpeak channel name or path, empty for the default channel";
  }
  return account?.service === "discord" ? "Discord voice channel id" : "Voice channel";
}

export function allowedEntries(allowed: readonly string[]): Settings[] {
  return allowed.map((id) => ({ id }));
}

export function allowedOf(entries: readonly Settings[]): string[] {
  return entries
    .map((entry) => (typeof entry.id === "string" ? entry.id.trim() : ""))
    .filter((id) => id !== "");
}
