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

/**
 * Whether anything hears `node`'s audio: any audio input, or an encoder whose baseband goes
 * somewhere other than a transmitter, or to one that is armed.
 */
export function reachesListener(
  patch: ResolvedPatch | null,
  nodes: readonly PatchNode[],
  sets: readonly Pick<DeviceSet, "transmit">[],
  node: string,
): boolean {
  const kindOf = (id: string) => nodes.find((candidate) => candidate.id === id)?.kind;
  const heard = (encoder: string): boolean =>
    reachesOf(patch, encoder, "baseband").some(
      (reach) => kindOf(reach.sink) !== "transmit" || transmitStatusOf(sets, reach.sink) !== null,
    );
  return reachesOf(patch, node, "audio").some(
    (reach) => kindOf(reach.sink) !== "encoder" || heard(reach.sink),
  );
}

/** A voice source's audio reaches, through an Encoder, a transmitter that is armed. */
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
