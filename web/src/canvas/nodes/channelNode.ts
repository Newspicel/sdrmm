import { heldByOf, resolvedNode } from "../../lib/resolved";
import type { ResolvedPatch, Runs, UnboundReason } from "../../lib/types";

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
  resolved: ResolvedPatch | null,
  faces: ReadonlyMap<number, string>,
): ReadonlyMap<number, string> {
  const locked = new Map<number, string>();
  for (const [channel, id] of faces) {
    const by = heldByOf(resolved, id);
    if (by !== null) {
      locked.set(channel, `tuned by ${by}`);
    } else if (resolvedNode(resolved, id)?.held?.by === "hand") {
      locked.set(channel, "held, unlock it on its node to tune");
    }
  }
  return locked;
}
