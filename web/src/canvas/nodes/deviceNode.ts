import { agcState } from "../../components/capabilities";
import { reachableHz } from "../../components/dial";
import type {
  AgcSetting,
  Capabilities,
  Coherence,
  DeviceInfo,
  DeviceRef,
  DeviceSet,
  DeviceSettings,
  PatchNode,
  Tuning,
  VirtualLane,
} from "../../lib/types";
import { forStream } from "../../lib/useDevicePatch";
import { rxStreamCount, streamLabel } from "../graph";

export const GENERATOR_PREFIX = "gen-";

export function pinnedRef(node: PatchNode): DeviceRef | null {
  return node.kind === "device" ? (node.data.device ?? null) : null;
}

export function isRecordingRef(reference: DeviceRef): boolean {
  return reference.backend === "recording";
}

export function isGeneratorRef(reference: DeviceRef): boolean {
  return reference.backend === "virtual" && (reference.key?.startsWith(GENERATOR_PREFIX) ?? false);
}

export function recordingRef(stem: string): DeviceRef {
  return { backend: "recording", key: stem };
}

export function generatorRef(nodeId: string): DeviceRef {
  return { backend: "virtual", key: `${GENERATOR_PREFIX}${nodeId.replaceAll(":", "-")}` };
}

export function deviceRefOf(info: DeviceInfo): DeviceRef {
  return {
    backend: info.driver,
    ...(info.serial == null ? {} : { serial: info.serial }),
    ...(info.serial == null || info.key.startsWith(`${info.serial}@`) ? { key: info.key } : {}),
  };
}

export function clippingLanes(set: DeviceSet): string[] | null {
  const lanes = set.clipping ?? [];
  if (lanes.length === 0) {
    return null;
  }
  const streams = rxStreamCount(set.capabilities);
  if (streams <= 1) {
    return [];
  }
  return lanes.map((lane) => streamLabel("iq", lane, streams));
}

export interface TunerDial {
  stream: number;
  port: string | null;
  hz: number;
}

export function tunerDials(set: DeviceSet): TunerDial[] {
  const capabilities = set.capabilities;
  const scope = capabilities.per_stream;
  const streams = rxStreamCount(capabilities);
  if (scope?.tuning !== true || streams < 2) {
    return [{ stream: 0, port: null, hz: set.settings.center_hz ?? 0 }];
  }
  return Array.from({ length: streams }, (_, stream) => ({
    stream,
    port: streamLabel("iq", stream, streams),
    hz: forStream(set.settings, stream, scope).center_hz ?? 0,
  }));
}

export function lanesMerged(set: DeviceSet): boolean {
  return set.capabilities.per_stream?.tuning === true && rxStreamCount(set.capabilities) > 1;
}

export function rxStreams(capabilities: Capabilities): number[] {
  return Array.from({ length: rxStreamCount(capabilities) }, (_, stream) => stream);
}

export function lanesAligned(set: DeviceSet): boolean {
  const dials = tunerDials(set);
  return dials.every((dial) => dial.hz === dials[0]?.hz);
}

export function tuneAllDelta(capabilities: Capabilities, hz: number): DeviceSettings {
  const center_hz = reachableHz(capabilities, hz);
  if (capabilities.per_stream?.tuning !== true) {
    return { center_hz, tuning: "manual" };
  }
  return {
    streams: rxStreams(capabilities).map((stream) => ({ stream, center_hz, tuning: "manual" })),
  };
}

export function tuningAllDelta(capabilities: Capabilities, tuning: Tuning): DeviceSettings {
  if (capabilities.per_stream?.tuning !== true) {
    return { tuning };
  }
  return { streams: rxStreams(capabilities).map((stream) => ({ stream, tuning })) };
}

export function allAutoTuning(set: DeviceSet): boolean {
  return rxStreams(set.capabilities).every((stream) => autoTuning(set, stream));
}

const BONDS: Record<Coherence, string | null> = {
  none: null,
  time_sync: "Shared clock",
  phase_coherent: "Phase coherent",
};

export function bondSaid(coherence: Coherence | undefined): string | null {
  return BONDS[coherence ?? "none"];
}

export function autoTuning(set: DeviceSet, stream = 0): boolean {
  const resolved = forStream(set.settings, stream, set.capabilities.per_stream);
  return (resolved.tuning ?? "auto") === "auto";
}

function virtualLane(set: DeviceSet, stream: number): VirtualLane | undefined {
  return (set.virtual_lanes ?? []).find((lane) => lane.stream === stream);
}

export function laneCenterHz(set: DeviceSet, stream: number): number | null {
  const virtual = virtualLane(set, stream);
  if (virtual !== undefined) {
    return virtual.center_hz;
  }
  return forStream(set.settings, stream, set.capabilities.per_stream).center_hz ?? null;
}

export function laneRateHz(set: DeviceSet, stream: number): number | undefined {
  return virtualLane(set, stream)?.sample_rate ?? set.settings.sample_rate ?? undefined;
}

export function tuneDelta(capabilities: Capabilities, stream: number, hz: number): DeviceSettings {
  const center_hz = reachableHz(capabilities, hz);
  return capabilities.per_stream?.tuning === true
    ? { streams: [{ stream, center_hz, tuning: "manual" }] }
    : { center_hz, tuning: "manual" };
}

export function tuningDelta(
  capabilities: Capabilities,
  stream: number,
  tuning: Tuning,
): DeviceSettings {
  return capabilities.per_stream?.tuning === true ? { streams: [{ stream, tuning }] } : { tuning };
}

export function laneAgc(set: DeviceSet, stream: number): AgcSetting {
  return agcState(set.capabilities, forStream(set.settings, stream, set.capabilities.per_stream));
}

export function agcDelta(
  capabilities: Capabilities,
  stream: number,
  agc: AgcSetting,
): DeviceSettings {
  return capabilities.per_stream?.agc === true ? { streams: [{ stream, agc }] } : { agc };
}

export function lossSaid(set: DeviceSet): string | null {
  const loss = set.loss;
  return loss == null ? null : `${Math.round(loss * 100)}%`;
}

export function radioAgc(set: DeviceSet): AgcSetting {
  const caps = set.capabilities;
  if (caps.per_stream?.agc !== true) {
    return agcState(caps, set.settings);
  }
  const lanes = Array.from({ length: rxStreamCount(caps) }, (_, stream) => laneAgc(set, stream));
  const running = lanes.find((lane) => lane.on);
  return running ?? { ...(lanes[0] ?? agcState(caps, set.settings)), on: false };
}

export function agcModeDelta(set: DeviceSet, mode: string): DeviceSettings {
  const caps = set.capabilities;
  if (caps.per_stream?.agc !== true) {
    return { agc: { on: true, mode } };
  }
  return {
    streams: Array.from({ length: rxStreamCount(caps) }, (_, stream) => ({
      stream,
      agc: { on: laneAgc(set, stream).on, mode },
    })),
  };
}

export function agcGainDb(set: DeviceSet, stream: number): number | null {
  const driven = set.capabilities.gains.filter((stage) => stage.agc?.kind !== "never");
  if (driven.length !== 1 || !laneAgc(set, stream).on) {
    return null;
  }
  return set.agc_gains?.find((reading) => reading.stream === stream)?.value_db ?? null;
}

export function lockStream(locked: readonly number[], stream: number, held: boolean): number[] {
  const others = locked.filter((candidate) => candidate !== stream);
  return held ? [...others, stream].toSorted((a, b) => a - b) : others;
}

export function lockAll(capabilities: Capabilities, held: boolean): number[] {
  return held ? rxStreams(capabilities) : [];
}

export function laneLocked(node: PatchNode | undefined, stream: number): boolean {
  return node?.kind === "device" && (node.data.locked_streams ?? []).includes(stream);
}

export function anyLocked(locked: readonly number[], capabilities: Capabilities): boolean {
  return rxStreams(capabilities).some((stream) => locked.includes(stream));
}

export interface Hearing {
  heard: number;
  total: number;
  tone: "ok" | "warn" | "danger";
}

export function hearing(set: DeviceSet): Hearing {
  const total = set.channels.length;
  const heard = set.channels.filter((channel) => !channel.out_of_band).length;
  const missing = set.status !== "running" || (total > 0 && heard === 0);
  return { heard, total, tone: missing ? "danger" : heard === total ? "ok" : "warn" };
}

export function refLabel(reference: DeviceRef): string {
  const identity = reference.key ?? reference.serial;
  return identity == null ? reference.backend : `${reference.backend} · ${identity}`;
}

const FAULTS: Record<string, string> = {
  unplugged: "is no longer attached. Plug it back in and it picks up where it left off.",
  in_use: "is open in another program. Close that one, and this radio comes back.",
  permissions:
    "may not be opened by this user. Open Check hardware for the USB permission line, which names the device node and the group that owns it.",
};

function listed(names: readonly string[]): string {
  return names.length === 1
    ? (names[0] ?? "")
    : `${names.slice(0, -1).join(", ")} and ${names.at(-1)}`;
}

export function refusalSaid(set: DeviceSet): string | null {
  const names = set.refused?.settings;
  if (set.error != null || names === undefined) {
    return null;
  }
  if (names.length === 0) {
    return "Radio refused the change";
  }
  return `Radio refused the new ${listed(names)}`;
}

export function faultSaid(set: DeviceSet): string | null {
  const said = set.fault == null ? undefined : FAULTS[set.fault];
  return said == null ? null : `${set.device.label} ${said}`;
}

export function capturesOn(set: DeviceSet): string[] {
  return [
    set.recording == null ? null : "a baseband recording",
    set.network_export == null ? null : "a network export",
    set.pre_roll == null ? null : "its pre-roll",
  ].filter((name): name is string => name !== null);
}

export function stopSaid(set: DeviceSet): string {
  const live = capturesOn(set);
  return live.length === 0
    ? "Close this radio and keep it on the node, wires and settings included"
    : `Closing this radio also stops ${listed(live)}`;
}
