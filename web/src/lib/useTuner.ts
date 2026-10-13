import { useWorkspaceContext, type Workspace } from "../canvas/context";
import { nodeOf } from "../canvas/graph";
import type { TuneTarget } from "../canvas/libraryTarget";
import { channelSettingsOf } from "./useChannelEdit";
import { forStream } from "./useDevicePatch";
import { useNodeTune } from "./useNodeTune";

export interface Tuner {
  tune: (hz: number) => void;
  frequencyHz: number | null;
  channelType: string | null;
  ready: boolean;
}

export function useTuner(target: TuneTarget | null): Tuner {
  const workspace = useWorkspaceContext();
  const tuneNode = useNodeTune();
  return {
    tune: (hz) => {
      if (target !== null) {
        tuneNode(target.node, hz);
      }
    },
    frequencyHz: target === null ? null : frequencyOf(workspace, target),
    channelType:
      target === null || target.kind === "device" ? null : channelTypeOf(workspace, target.node),
    ready: target !== null,
  };
}

export function sameMode(mode: string | null | undefined, channelType: string | null): boolean {
  return mode == null || mode === "" || mode.toLowerCase() === channelType?.toLowerCase();
}

function frequencyOf(workspace: Workspace, target: TuneTarget): number | null {
  return target.kind === "device"
    ? (forStream(target.set.settings, 0, target.set.capabilities.per_stream).center_hz ?? null)
    : (channelSettingsOf(workspace, target.node)?.frequency_hz ?? null);
}

function channelTypeOf(workspace: Workspace, node: string): string | null {
  const patch = nodeOf(workspace.graph, node);
  return patch?.kind === "channel" ? patch.data.channel_type : null;
}
