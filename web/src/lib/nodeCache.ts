import type { QueryClient } from "@tanstack/react-query";
import { withSavedChannel } from "../canvas/savedChannels";
import { mergeChannelSettings } from "../components/channelSettings";
import { CHANNEL_TYPES_KEY, RESOLVED_KEY, STATE_KEY, WORKSPACES_KEY } from "./api";
import type {
  ChannelInfo,
  ChannelSettings,
  ChannelTypesResponse,
  DeviceSet,
  ResolvedState,
  StateSnapshot,
  WorkspaceDetail,
  WorkspacesResponse,
} from "./types";

export function cachedRadio(queryClient: QueryClient, node: string): DeviceSet | undefined {
  const runs = queryClient
    .getQueryData<ResolvedState>(RESOLVED_KEY)
    ?.patch.nodes.find((held) => held.id === node)?.runs;
  if (runs?.runs !== "radio") {
    return undefined;
  }
  return queryClient
    .getQueryData<StateSnapshot>(STATE_KEY)
    ?.device_sets.find((set) => set.id === runs.device_set);
}

export function cachedChannel(queryClient: QueryClient, node: string): ChannelInfo | undefined {
  return queryClient
    .getQueryData<StateSnapshot>(STATE_KEY)
    ?.device_sets.flatMap((set) => set.channels)
    .find((channel) => channel.node === node);
}

function activeDetailKey(queryClient: QueryClient): readonly unknown[] | null {
  const active = queryClient.getQueryData<WorkspacesResponse>(WORKSPACES_KEY)?.active;
  return active == null ? null : [...WORKSPACES_KEY, active];
}

export function channelSettingsAt(
  queryClient: QueryClient,
  node: string,
): ChannelSettings | undefined {
  const live = cachedChannel(queryClient, node)?.settings;
  if (live !== undefined) {
    return live;
  }
  const key = activeDetailKey(queryClient);
  const detail = key === null ? undefined : queryClient.getQueryData<WorkspaceDetail>(key);
  const saved = detail?.state?.channels?.find((held) => held.node === node)?.settings;
  if (saved !== undefined) {
    return saved;
  }
  const drawn = detail?.snapshot.graph.nodes.find((held) => held.id === node);
  const type = drawn?.kind === "channel" ? drawn.data.channel_type : null;
  return (
    queryClient
      .getQueryData<ChannelTypesResponse>(CHANNEL_TYPES_KEY)
      ?.types.find((descriptor) => descriptor.type_id === type)?.defaults ?? undefined
  );
}

export function showChannel(
  queryClient: QueryClient,
  node: string,
  settings: ChannelSettings,
): void {
  const live = cachedChannel(queryClient, node);
  if (live !== undefined) {
    queryClient.setQueryData<StateSnapshot>(STATE_KEY, (prev) =>
      prev === undefined
        ? prev
        : {
            ...prev,
            device_sets: prev.device_sets.map((set) => ({
              ...set,
              channels: set.channels.map((channel) =>
                channel.node === node ? { ...channel, settings } : channel,
              ),
            })),
          },
    );
    return;
  }
  const key = activeDetailKey(queryClient);
  if (key !== null) {
    queryClient.setQueryData<WorkspaceDetail>(key, (held) =>
      held === undefined ? held : withSavedChannel(held, node, settings),
    );
  }
}

export function editedChannel(
  current: ChannelSettings,
  edit: Partial<ChannelSettings> | ((current: ChannelSettings) => Partial<ChannelSettings>),
): ChannelSettings {
  return mergeChannelSettings(current, typeof edit === "function" ? edit(current) : edit);
}
