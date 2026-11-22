import type { Settings } from "../../lib/settingsForm";
import type {
  DeviceSet,
  EncoderDescriptor,
  PatchNode,
  TransmitSettings,
  TransmitStatus,
  VoiceGateSettings,
} from "../../lib/types";

export const MIN_LEVEL_DB = -60;
export const MAX_TIMEOUT_S = 3_600;

const NO_SETTINGS: Settings = {};

/** The live transmitter this node armed, whichever radio carries it. */
export function transmitStatusOf(
  sets: readonly Pick<DeviceSet, "transmit">[],
  node: string,
): TransmitStatus | null {
  for (const set of sets) {
    if (set.transmit != null && set.transmit.node === node) {
      return set.transmit;
    }
  }
  return null;
}

export function withTransmitSettings(node: PatchNode, next: Partial<TransmitSettings>): PatchNode {
  return node.kind === "transmit" ? { ...node, data: { ...node.data, ...next } } : node;
}

export function withEncoderSettings(node: PatchNode, settings: Settings): PatchNode {
  return node.kind === "encoder" ? { ...node, data: { ...node.data, settings } } : node;
}

/** What the encoder runs on: its own settings, else its defaults. */
export function encoderSettings(
  stored: unknown,
  descriptor: EncoderDescriptor | undefined,
): Settings {
  if (typeof stored === "object" && stored !== null) {
    return stored as Settings;
  }
  return descriptor?.defaults ?? NO_SETTINGS;
}

/** An encoder's signal that spills past the width its mode is meant to stay inside. */
export function tooWide(status: Pick<TransmitStatus, "occupied_hz" | "channel_hz">): boolean {
  return (
    status.occupied_hz != null &&
    status.channel_hz != null &&
    status.occupied_hz > status.channel_hz
  );
}

export function withMicrophone(
  node: PatchNode,
  next: Partial<Extract<PatchNode, { kind: "microphone" }>["data"]>,
): PatchNode {
  return node.kind === "microphone" ? { ...node, data: { ...node.data, ...next } } : node;
}

/** A gate edit on a microphone, whose gate mode is its `send`. */
export function withMicrophoneGate(
  node: PatchNode,
  { mode, ...levels }: Partial<VoiceGateSettings>,
): PatchNode {
  return withMicrophone(node, mode === undefined ? levels : { ...levels, send: mode });
}
