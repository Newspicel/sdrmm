import { create } from "zustand";
import type { PortRef, PortSpec, WireVerdict } from "./types";

export const NOT_OFFERED = "That input does not take this wire";
export const NO_PORT = "That port does not exist";
export const SELF_WIRE = "A node cannot wire to itself";

export type Verdicts = ReadonlyMap<string, string | null>;

export interface WiringStore {
  from: PortRef | null;
  verdicts: Verdicts | null;
  start: (from: PortRef | null) => void;
  settle: (from: PortRef, verdicts: readonly WireVerdict[]) => void;
  clear: () => void;
}

export function portKey(port: PortRef): string {
  return `${port.node}\n${port.port}`;
}

function sameFrom(held: PortRef | null, from: PortRef): boolean {
  return held !== null && portKey(held) === portKey(from);
}

export const useWiringStore = create<WiringStore>((set) => ({
  from: null,
  verdicts: null,
  start: (from) => set({ from, verdicts: null }),
  settle: (from, verdicts) =>
    set((state) =>
      sameFrom(state.from, from)
        ? {
            verdicts: new Map(
              verdicts.map((verdict) => [portKey(verdict.to), verdict.refusal ?? null]),
            ),
          }
        : state,
    ),
  clear: () => set({ from: null, verdicts: null }),
}));

export interface WireEnds {
  from: PortRef;
  to: PortRef;
  out: PortSpec | undefined;
  input: PortSpec | undefined;
}

export function wireRefusal(verdicts: Verdicts | null, ends: WireEnds): string | null {
  if (ends.from.node === ends.to.node) {
    return SELF_WIRE;
  }
  if (ends.out === undefined || ends.input === undefined) {
    return NO_PORT;
  }
  if (ends.out.port_type !== ends.input.port_type) {
    return `${ends.out.port_type} cannot feed ${ends.input.port_type}`;
  }
  if (verdicts === null) {
    return null;
  }
  const verdict = verdicts.get(portKey(ends.to));
  return verdict === undefined ? NOT_OFFERED : verdict;
}
