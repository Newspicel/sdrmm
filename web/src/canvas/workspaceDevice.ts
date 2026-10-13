import { type Controlled, controlledChannel, runningSet } from "../lib/resolved";
import type { DeviceSet } from "../lib/types";
import type { Workspace } from "./context";
import { type IqLane, iqSourceOf } from "./lanes";

export function runningSetOf(workspace: Workspace, node: string): DeviceSet | null {
  return runningSet(workspace.resolved, workspace.deviceSets, node);
}

export function controlledOf(workspace: Workspace, node: string): Controlled | null {
  return controlledChannel(workspace.resolved, workspace.deviceSets, node);
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
