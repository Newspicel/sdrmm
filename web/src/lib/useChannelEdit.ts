import type { Workspace } from "../canvas/context";
import { descriptorOf, nodeOf } from "../canvas/graph";
import { runningSetOf } from "../canvas/workspaceDevice";
import type { ChannelSettings } from "./types";
import { type ChannelEdit, useChannelPatch } from "./useChannelPatch";

export interface LiveChannel {
  deviceSet: number;
  id: number;
}

export function liveChannelOf(workspace: Workspace, node: string): LiveChannel | null {
  const set = runningSetOf(workspace, node);
  const channel = workspace.channels.get(node);
  return set === null || channel === undefined ? null : { deviceSet: set.id, id: channel.id };
}

export function channelSettingsOf(workspace: Workspace, node: string): ChannelSettings | null {
  const patch = nodeOf(workspace.graph, node);
  const defaults =
    patch === undefined ? undefined : descriptorOf(workspace.context, patch)?.defaults;
  return (
    workspace.channels.get(node)?.settings ?? workspace.savedChannels.get(node) ?? defaults ?? null
  );
}

export function useChannelEdit(): (node: string, edit: ChannelEdit) => void {
  return useChannelPatch().applyEdit;
}
