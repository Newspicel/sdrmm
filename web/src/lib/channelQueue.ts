import type { ChannelSettings } from "./types";

export interface ChannelQueuePorts {
  read: (node: string) => ChannelSettings | undefined;
  show: (node: string, settings: ChannelSettings) => void;
  send: (node: string, settings: ChannelSettings, base: ChannelSettings) => Promise<unknown>;
  failed: (error: unknown) => void;
  drained: () => void;
}

interface Lane {
  base: ChannelSettings;
  desired: ChannelSettings;
}

/// One PATCH per channel in flight. Edits made meanwhile fold into `desired` and go out
/// next, against the settings the server last acknowledged, so the newest edit always wins.
export function createChannelQueue(
  ports: ChannelQueuePorts,
): (node: string, edit: (current: ChannelSettings) => ChannelSettings) => void {
  const lanes = new Map<string, Lane>();

  const settle = (node: string, lane: Lane, sent: ChannelSettings, ok: boolean): void => {
    if (ok) {
      lane.base = sent;
    }
    if (lane.desired === sent) {
      lanes.delete(node);
      ports.drained();
      return;
    }
    ports.show(node, lane.desired);
    pump(node, lane);
  };

  const pump = (node: string, lane: Lane): void => {
    const sent = lane.desired;
    ports.send(node, sent, lane.base).then(
      () => settle(node, lane, sent, true),
      (error: unknown) => {
        ports.failed(error);
        settle(node, lane, sent, false);
      },
    );
  };

  return (node, edit) => {
    const lane = lanes.get(node);
    const current = lane?.desired ?? ports.read(node);
    if (current === undefined) {
      return;
    }
    const next = edit(current);
    ports.show(node, next);
    if (lane !== undefined) {
      lane.desired = next;
      return;
    }
    const fresh: Lane = { base: current, desired: next };
    lanes.set(node, fresh);
    pump(node, fresh);
  };
}
