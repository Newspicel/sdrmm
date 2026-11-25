import { feedsOf, reachesOf } from "../../lib/resolved";
import type { ResolvedPatch } from "../../lib/types";
import { useWorkspaceSelect } from "../context";

/** A port the node needs a wire on. `out` marks an output that has to reach something. */
export interface Want {
  port: string;
  out?: boolean;
}

export const NO_WANTS: readonly Want[] = [];

/** A node that sweeps a decoder: the decoder it drives and the lane it watches. */
export const SWEEP_WANTS: readonly Want[] = [{ port: "control", out: true }, { port: "iq" }];

/**
 * True while a port the node needs has no wire. The empty port already shows it, so a face
 * never says so in words; it only holds back what cannot run.
 */
export function unwiredOf(
  patch: ResolvedPatch | null,
  node: string,
  wants: readonly Want[],
): boolean {
  return wants.some((want) => !wired(patch, node, want));
}

export function useUnwired(node: string, wants: readonly Want[]): boolean {
  return useWorkspaceSelect((workspace) => unwiredOf(workspace.resolved, node, wants));
}

function wired(patch: ResolvedPatch | null, node: string, want: Want): boolean {
  return want.out === true
    ? reachesOf(patch, node, want.port).length > 0
    : feedsOf(patch, node, want.port).length > 0;
}
