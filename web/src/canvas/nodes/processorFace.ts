import { GATE_TEXT, processorStatusOf } from "../../lib/arrays";
import { isStale, type ProcessorState } from "../../lib/processors";
import type {
  ArrayStatus,
  ArrayTuningMode,
  PatchCatalog,
  PatchGraph,
  PatchNode,
  ProcessorFacets,
  ProcessorGate,
  ProcessorStatus,
  ResolvedPatch,
} from "../../lib/types";
import { nodeOf, patchNode } from "../graph";
import { arrayOf, wiredLaneCount } from "../lanes";
import { processorTypeOf } from "../newNode";
import type { Chip } from "./ProcessorHealth";

export const NO_ARRAY = "no array";
export const STALE = "stale";
export const NO_CATALOG = "Node types not loaded";
export const SETTINGS_ACTION = "Settings";
export const NOT_LOADED = "Not loaded";
const FALLBACK_TITLE = "Processor";
const NO_FACETS: ProcessorFacets = { views: [], actions: [], tuning: null };

export const STALE_CHIP: Chip = {
  label: "Stale",
  title: "No reading for three report periods",
  danger: true,
};

export interface ProcessorKindInfo {
  title: string;
  facets: ProcessorFacets;
  loaded: boolean;
}

/** What the catalog says a processor node draws: a built-in's own entry, or its plugin type. */
export function processorKindOf(catalog: PatchCatalog, node: PatchNode): ProcessorKindInfo {
  const entry = catalog.nodes.find((type) => type.kind === node.kind);
  if (entry?.needs_processor_type !== true) {
    return {
      title: entry?.name ?? FALLBACK_TITLE,
      facets: entry?.processor ?? NO_FACETS,
      loaded: entry !== undefined,
    };
  }
  const typeId = node.kind === "processor" ? (node.data?.processor_type ?? "") : "";
  const type = processorTypeOf(catalog, typeId);
  return {
    title: type?.name ?? (typeId === "" ? FALLBACK_TITLE : typeId),
    facets: type?.facets ?? NO_FACETS,
    loaded: type !== undefined,
  };
}

const TUNING_ASK: Readonly<Record<ArrayTuningMode, { chip: Chip; label: string; title: string }>> =
  {
    spread: {
      chip: {
        label: "Needs spread",
        title: "Needs the array's lanes tuned side by side",
        danger: true,
      },
      label: "Spread array",
      title: "Tune the wired array's lanes side by side",
    },
    together: {
      chip: {
        label: "Needs together",
        title: "Needs the array's lanes on one frequency",
        danger: true,
      },
      label: "Tune together",
      title: "Tune the wired array's lanes to one frequency",
    },
  };

export function tuningAskOf(mode: ArrayTuningMode) {
  return TUNING_ASK[mode];
}

/** The tuning the processor needs when its wired array tunes otherwise. */
export function tuningAsk(
  graph: PatchGraph,
  array: string | null,
  tuning: ArrayTuningMode | null | undefined,
): ArrayTuningMode | null {
  if (tuning == null || array === null) {
    return null;
  }
  const node = nodeOf(graph, array);
  if (node?.kind !== "array") {
    return null;
  }
  return (node.data.tuning ?? "together") === tuning ? null : tuning;
}

export function tuneArrayEdit(
  graph: PatchGraph,
  array: string | null,
  tuning: ArrayTuningMode,
): PatchGraph | null {
  if (array === null) {
    return null;
  }
  return patchNode(graph, array, (node) =>
    node.kind === "array" ? { ...node, data: { ...node.data, tuning } } : node,
  );
}

export function faceChips(
  state: ProcessorState | undefined,
  stale: boolean,
  ask: ArrayTuningMode | null,
): Chip[] {
  return [
    ...(ask === null ? [] : [TUNING_ASK[ask].chip]),
    ...(stale ? [STALE_CHIP] : []),
    ...(state?.view.flags ?? []),
  ];
}

export function processorStale(state: ProcessorState | undefined, now: number): boolean {
  return state !== undefined && isStale(state.receivedAt, now, state.periodMs);
}

export function processorGate(status: ArrayStatus | undefined, node: string): ProcessorGate | null {
  return processorStatusOf(status, node)?.gated ?? null;
}

export interface Subtitle {
  text: string;
  warn: boolean;
}

export function processorSubtitle(
  resolved: ResolvedPatch | null,
  node: string,
  status: ArrayStatus | undefined,
  state: ProcessorState | undefined,
  now: number,
): Subtitle {
  const array = arrayOf(resolved, node);
  if (array === null) {
    return { text: NO_ARRAY, warn: false };
  }
  const gate = processorGate(status, node);
  if (gate !== null) {
    return { text: GATE_TEXT[gate].toLowerCase(), warn: true };
  }
  const headline = state?.view.headline ?? null;
  if (headline?.warn === true) {
    return { text: headline.text, warn: true };
  }
  if (processorStale(state, now)) {
    return { text: STALE, warn: true };
  }
  if (headline !== null) {
    return { text: headline.text, warn: false };
  }
  const lanes = processorLanes(resolved, array, status);
  return { text: `${lanes} ${lanes === 1 ? "lane" : "lanes"}`, warn: false };
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
