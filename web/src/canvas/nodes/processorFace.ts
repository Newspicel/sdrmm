import { GATE_TEXT, processorStatusOf } from "../../lib/arrays";
import { isStale, type ProcessorState } from "../../lib/processors";
import type { ArrayStatus, ProcessorGate, ProcessorStatus, ResolvedPatch } from "../../lib/types";
import { arrayOf, wiredLaneCount } from "../lanes";

export const NO_ARRAY = "no array";
export const STALE = "stale";
export const NO_CATALOG = "Node types not loaded";
export const SETTINGS_ACTION = "Settings";

export function processorGate(status: ArrayStatus | undefined, node: string): ProcessorGate | null {
  return processorStatusOf(status, node)?.gated ?? null;
}

export function processorSubtitle(
  resolved: ResolvedPatch | null,
  node: string,
  status: ArrayStatus | undefined,
  state: ProcessorState | undefined,
  now: number,
  periodMs: number,
): string {
  const array = arrayOf(resolved, node);
  if (array === null) {
    return NO_ARRAY;
  }
  const gate = processorGate(status, node);
  if (gate !== null) {
    return GATE_TEXT[gate].toLowerCase();
  }
  if (state !== undefined && isStale(state.receivedAt, now, periodMs)) {
    return STALE;
  }
  const lanes = processorLanes(resolved, array, status);
  return `${lanes} ${lanes === 1 ? "lane" : "lanes"}`;
}

export function processorLanes(
  resolved: ResolvedPatch | null,
  array: string | null,
  status: ArrayStatus | undefined,
): number {
  if (array === null) {
    return 0;
  }
  return status?.lanes.length ?? wiredLaneCount(resolved, array);
}

export interface FaultRow {
  label: string;
  count: number;
  title: string;
}

export function processorFaults(status: ProcessorStatus | null): FaultRow[] {
  if (status === null) {
    return [];
  }
  return [
    { label: "Drops", count: status.dropped_samples, title: "Samples dropped" },
    { label: "Lost", count: status.dropped_reports, title: "Reports lost" },
    {
      label: "Cut",
      count: (status.truncated ?? 0) + status.lane_overflows,
      title: "Results cut at their limit",
    },
    { label: "Fails", count: status.solver_failures, title: "Solver failures" },
    { label: "Mismatch", count: status.lane_mismatch, title: "Blocks with the wrong lane count" },
  ].filter((row) => row.count > 0);
}

export function ageLabel(receivedAt: number | undefined, now: number): string {
  if (receivedAt === undefined) {
    return "-";
  }
  const seconds = Math.max(0, now - receivedAt) / 1_000;
  if (seconds < 10) {
    return `${seconds.toFixed(1)} s`;
  }
  if (seconds < 60) {
    return `${Math.floor(seconds)} s`;
  }
  return `${Math.floor(seconds / 60)} min`;
}
