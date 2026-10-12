import { controlledBy, radioOf } from "../lib/resolved";
import type { ChannelInfo, DeviceSet } from "../lib/types";
import type { Workspace } from "./context";
import { type IqLane, iqSourceOf } from "./lanes";

export function deviceSetOf(workspace: Workspace, node: string): DeviceSet | null {
  const owner = radioOf(workspace.resolved, node);
  return owner === null ? null : (workspace.devices.get(owner) ?? null);
}

export function laneOf(workspace: Workspace, node: string): IqLane | null {
  const wired = iqSourceOf(workspace.resolved, node);
  const owner = workspace.owners.get(node);
  const channel = workspace.channels.get(node);
  if (owner !== undefined && channel !== undefined) {
    const stream = channel.stream ?? 0;
    return wired?.source === owner && wired.stream === stream ? wired : { source: owner, stream };
  }
  return wired;
}

export interface Decoder {
  node: string;
  set: DeviceSet;
  channel: ChannelInfo;
}

export function decoderOf(workspace: Workspace, node: string): Decoder | null {
  const driven = controlledBy(workspace.resolved, node);
  if (driven === null) {
    return null;
  }
  const channel = workspace.channels.get(driven);
  const set = deviceSetOf(workspace, driven);
  return channel === undefined || set === null ? null : { node: driven, set, channel };
}
