import type { DeviceInfo, DeviceRef, PatchGraph, ResolvedPatch } from "../../lib/types";
import { tuningLocked } from "../graph";
import { iqLanesOf } from "../lanes";
import { nodeDeviceRef, refMatches } from "./deviceNode";

export type ChannelBinding =
  | "unwired"
  | "no-radio"
  | "radio-absent"
  | "radio-closed"
  | "not-started";

export function radioRefsOf(
  graph: PatchGraph,
  resolved: ResolvedPatch | null,
  node: string,
): DeviceRef[] {
  const refs: DeviceRef[] = [];
  for (const lane of iqLanesOf(resolved, node)) {
    const found = graph.nodes.find((candidate) => candidate.id === lane.source);
    const reference = found === undefined ? null : nodeDeviceRef(found);
    if (reference !== null) {
      refs.push(reference);
    }
  }
  return refs;
}

export function lockedChannels(
  graph: PatchGraph,
  faces: ReadonlyMap<number, string>,
): ReadonlySet<number> {
  const locked = new Set<number>();
  for (const [channel, node] of faces) {
    if (tuningLocked(graph, node)) {
      locked.add(channel);
    }
  }
  return locked;
}

export function radioIsAttached(
  references: readonly DeviceRef[],
  attached: readonly DeviceInfo[],
): boolean {
  return references.some((reference) => attached.some((device) => refMatches(reference, device)));
}

export function channelBinding(input: {
  wired: boolean;
  open: boolean;
  named: boolean;
  attached: boolean;
}): ChannelBinding {
  if (!input.wired) {
    return "unwired";
  }
  if (input.open) {
    return "not-started";
  }
  if (!input.named) {
    return "no-radio";
  }
  return input.attached ? "radio-closed" : "radio-absent";
}

const HINTS: Record<ChannelBinding, string> = {
  unwired: "Wire a device's IQ in",
  "no-radio": "Pick a radio on the device node",
  "radio-absent": "Its radio is not connected",
  "radio-closed": "Its radio is not open",
  "not-started": "Not started on the radio yet",
};

const ACTIONS: Partial<Record<ChannelBinding, string>> = {
  "radio-closed": "Open radio",
  "not-started": "Start channel",
};

const STATUS: Record<ChannelBinding, string> = {
  unwired: "unwired",
  "no-radio": "no radio",
  "radio-absent": "radio missing",
  "radio-closed": "radio closed",
  "not-started": "not started",
};

export function channelBindingStatus(binding: ChannelBinding): string {
  return STATUS[binding];
}

export function channelBindingHint(binding: ChannelBinding): string {
  return HINTS[binding];
}

export function channelBindingAction(binding: ChannelBinding): string | null {
  return ACTIONS[binding] ?? null;
}
