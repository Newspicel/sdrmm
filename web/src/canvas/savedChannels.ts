import { WORKSPACE_STATE_VERSION } from "../lib/limits";
import type { ChannelSettings, WorkspaceDetail } from "../lib/types";

export function savedChannelsOf(
  detail: WorkspaceDetail | null,
): ReadonlyMap<string, ChannelSettings> {
  const held = new Map<string, ChannelSettings>();
  for (const channel of detail?.state?.channels ?? []) {
    held.set(channel.node, channel.settings);
  }
  return held;
}

/// Mirrors what the server will hold, so a control does not snap back while the write is in air.
export function withSavedChannel(
  detail: WorkspaceDetail,
  node: string,
  settings: ChannelSettings,
): WorkspaceDetail {
  const state = detail.state ?? { version: WORKSPACE_STATE_VERSION, devices: [], channels: [] };
  const channels = state.channels ?? [];
  return {
    ...detail,
    state: {
      ...state,
      channels: channels.some((held) => held.node === node)
        ? channels.map((held) => (held.node === node ? { ...held, settings } : held))
        : [...channels, { node, settings }],
    },
  };
}
