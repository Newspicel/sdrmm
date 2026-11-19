import { reachesOf } from "../../lib/resolved";
import type { PatchNode, ResolvedPatch, TransmitStatus } from "../../lib/types";

export type Air = "armed" | "on air";

/** Where a transmitter stands, for the header: nothing while disarmed. */
export function airOf(status: TransmitStatus | null): Air | null {
  if (status === null) {
    return null;
  }
  return status.phase === "on_air" ? "on air" : "armed";
}

/** The transmitter an encoder's baseband reaches, as the server resolved the wires. */
export function transmitterOf(
  patch: ResolvedPatch | null,
  nodes: readonly PatchNode[],
  node: string,
): string | null {
  for (const reach of reachesOf(patch, node, "baseband")) {
    if (nodes.some((candidate) => candidate.id === reach.sink && candidate.kind === "transmit")) {
      return reach.sink;
    }
  }
  return null;
}
