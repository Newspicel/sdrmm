import type { Settings } from "../../lib/settingsForm";
import type {
  ChannelDescriptor,
  DeviceSet,
  PatchNode,
  TransmitSettings,
  TransmitStatus,
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

/** Decoders that bring a modulator, in the catalog's order. */
export function modulatorTypes(types: readonly ChannelDescriptor[]): ChannelDescriptor[] {
  return types.filter((type) => type.can_transmit === true);
}

/** A new mode starts on its own defaults: another decoder's settings mean nothing to it. */
export function withModulatorType(node: PatchNode, channelType: string): PatchNode {
  return node.kind === "modulator" && node.data.channel_type !== channelType
    ? { ...node, data: { channel_type: channelType } }
    : node;
}

export function withModulatorSettings(node: PatchNode, settings: Settings): PatchNode {
  return node.kind === "modulator" ? { ...node, data: { ...node.data, settings } } : node;
}

/** What the modulator runs on: its own settings, else the decoder's defaults. */
export function modulatorSettings(
  stored: unknown,
  descriptor: ChannelDescriptor | undefined,
): Settings {
  if (typeof stored === "object" && stored !== null) {
    return stored as Settings;
  }
  return descriptor?.defaults?.params.settings ?? NO_SETTINGS;
}

/** A modulator's signal that spills past the width its mode is meant to stay inside. */
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

export function peakText(status: TransmitStatus): string {
  return status.peak_db == null ? "none" : `${status.peak_db.toFixed(1)} dBFS`;
}
