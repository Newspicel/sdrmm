import { create } from "zustand";
import { omitNodes } from "./byNode";
import type { ChannelStatus, Field, ProcessorView, ServerEvent } from "./types";

export interface ProcessorState {
  view: ProcessorView;
  status?: ChannelStatus;
  receivedAt: number;
  /** The gap before this reading, so staleness follows each processor's own pace. */
  periodMs: number;
}

export interface ProcessorStore {
  byNode: Readonly<Record<string, ProcessorState>>;
  observe: (event: ServerEvent) => void;
  forget: (nodes: readonly string[]) => void;
  reset: () => void;
}

export const STALE_FLOOR_MS = 2_000;
const NO_VIEW: ProcessorView = {};

export const useProcessorStore = create<ProcessorStore>((set) => ({
  byNode: {},
  observe: (event) => {
    if (event.type !== "ProcessorUpdate") {
      return;
    }
    const { node, status, view } = event.data;
    const receivedAt = Date.now();
    set((state) => {
      const previous = state.byNode[node];
      const periodMs = previous === undefined ? 0 : receivedAt - previous.receivedAt;
      return {
        byNode: {
          ...state.byNode,
          [node]: { view: view ?? NO_VIEW, status, receivedAt, periodMs },
        },
      };
    });
  },
  forget: (nodes) => set((state) => ({ byNode: omitNodes(state.byNode, nodes) })),
  reset: () => set({ byNode: {} }),
}));

export function isStale(receivedAt: number, now: number, periodMs: number): boolean {
  return now - receivedAt > Math.max(3 * periodMs, STALE_FLOOR_MS);
}

const NO_READOUTS: readonly Field[] = [];

export function readoutsOf(state: ProcessorState | undefined): readonly Field[] {
  return state?.status?.readouts ?? NO_READOUTS;
}
