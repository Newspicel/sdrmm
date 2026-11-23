import { reachesOf } from "../../lib/resolved";
import type { DeviceSet, PatchNode, ResolvedPatch, TransmitStatus } from "../../lib/types";
import { transmitStatusOf } from "./transmit";

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

/** A voice source's audio reaches, through an Encoder, a transmitter that is armed. */
/** Whether anything hears `node`'s audio: an armed transmitter, or any other audio input. */
export function reachesListener(
  patch: ResolvedPatch | null,
  nodes: readonly PatchNode[],
  sets: readonly Pick<DeviceSet, "transmit">[],
  node: string,
): boolean {
  const encoder = (id: string): boolean =>
    nodes.some((candidate) => candidate.id === id && candidate.kind === "encoder");
  return (
    reachesOf(patch, node, "audio").some((reach) => !encoder(reach.sink)) ||
    reachesArmedTransmitter(patch, nodes, sets, node)
  );
}

export function reachesArmedTransmitter(
  patch: ResolvedPatch | null,
  nodes: readonly PatchNode[],
  sets: readonly Pick<DeviceSet, "transmit">[],
  node: string,
): boolean {
  return reachesOf(patch, node, "audio").some((reach) => {
    const encoder = nodes.some(
      (candidate) => candidate.id === reach.sink && candidate.kind === "encoder",
    );
    const transmitter = encoder ? transmitterOf(patch, nodes, reach.sink) : null;
    return transmitter !== null && transmitStatusOf(sets, transmitter) !== null;
  });
}
