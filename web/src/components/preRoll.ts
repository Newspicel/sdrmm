import type { DeviceSet, PreRollStatus } from "../lib/types";

export const MIN_PRE_ROLL_S = 1;
export const MAX_PRE_ROLL_S = 120;
export const DEFAULT_PRE_ROLL_S = 10;
export const PRE_ROLL_BYTES_PER_SAMPLE = 8;

export type PreRollPhase =
  | { kind: "unavailable" }
  | { kind: "idle" }
  | { kind: "held"; status: PreRollStatus }
  | { kind: "capturing"; status: PreRollStatus }
  | { kind: "busy"; owner: string };

export function preRollPhase(
  set: { status: string; pre_roll?: PreRollStatus | null } | null,
  node: string,
): PreRollPhase {
  if (set === null || set.status !== "running") {
    return { kind: "unavailable" };
  }
  const held = set.pre_roll;
  if (held == null) {
    return { kind: "idle" };
  }
  if (held.node !== node) {
    return { kind: "busy", owner: held.node };
  }
  return held.capture == null
    ? { kind: "held", status: held }
    : { kind: "capturing", status: held };
}

export function preRollFill(status: PreRollStatus): number {
  return status.capacity_samples === 0
    ? 0
    : Math.min(1, status.held_samples / status.capacity_samples);
}

export function heldSeconds(status: PreRollStatus): number {
  return status.sample_rate === 0 ? 0 : status.held_samples / status.sample_rate;
}

export function preRollOf(set: DeviceSet | null): PreRollStatus | null {
  return set?.pre_roll ?? null;
}
