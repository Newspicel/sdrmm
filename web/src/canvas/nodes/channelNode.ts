import type { PatchGraph, Runs, UnboundReason } from "../../lib/types";

export interface Unbound {
  status: string | null;
  hint: string;
  action: string | null;
}

const UNBOUND: Record<UnboundReason, Unbound> = {
  not_wired: { status: null, hint: "Wire a device's IQ in", action: null },
  no_radio: { status: "no radio", hint: "Pick a radio on the device node", action: null },
  radio_absent: { status: "radio missing", hint: "Its radio is not connected", action: null },
  radio_closed: { status: "radio closed", hint: "Its radio is not open", action: "Open radio" },
  not_placed: {
    status: "not started",
    hint: "Not started on the radio yet",
    action: "Start channel",
  },
  not_running: { status: "not running", hint: "Its source is not running yet", action: null },
};

export function unboundOf(runs: Runs | undefined): Unbound | null {
  return runs?.runs === "unbound" ? UNBOUND[runs.reason] : null;
}

export function lockedChannels(
  graph: PatchGraph,
  faces: ReadonlyMap<number, string>,
): ReadonlySet<number> {
  const locked = new Set<number>();
  for (const [channel, id] of faces) {
    const node = graph.nodes.find((candidate) => candidate.id === id);
    if (node?.kind === "channel" && node.data.tuning_locked === true) {
      locked.add(channel);
    }
  }
  return locked;
}
