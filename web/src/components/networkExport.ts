import { nodeAction } from "../lib/api";
import { pushToast } from "../lib/toasts";
import type { NetworkExportAction, NetworkExportStatus } from "../lib/types";

export type NetworkExportControl =
  | { kind: "unavailable" }
  | { kind: "ready" }
  | { kind: "active"; status: NetworkExportStatus }
  | { kind: "busy"; owner: string };

export interface NetworkExportSource {
  running: boolean;
  active?: NetworkExportStatus | null;
}

export function deviceExportSource(
  set: { status: string; network_export?: NetworkExportStatus | null } | null,
): NetworkExportSource | null {
  return set === null ? null : { running: set.status === "running", active: set.network_export };
}

export function channelExportSource(
  set: { status: string } | null,
  channel: { network_export?: NetworkExportStatus | null } | null,
): NetworkExportSource | null {
  return set === null || channel === null
    ? null
    : { running: set.status === "running", active: channel.network_export };
}

export function deriveNetworkExportControl(
  source: NetworkExportSource | null,
  node: string,
): NetworkExportControl {
  if (source === null || !source.running) {
    return { kind: "unavailable" };
  }
  const active = source.active;
  if (active == null) {
    return { kind: "ready" };
  }
  return active.node === node
    ? { kind: "active", status: active }
    : { kind: "busy", owner: active.node };
}

export function networkExportMutationOptions(
  node: string,
  request: typeof nodeAction = nodeAction,
  notify: (message: string) => void = pushToast,
) {
  return {
    mutationFn: (action: NetworkExportAction) =>
      request(node, { action: "network_export", with: action }),
    onError: (error: Error) => notify(error.message),
  };
}

export function networkExportControlsLocked(
  control: NetworkExportControl,
  pending: boolean,
): boolean {
  return control.kind === "active" || pending;
}
