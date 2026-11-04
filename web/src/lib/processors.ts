import { create } from "zustand";
import { omitNodes } from "./byNode";
import type {
  ChannelStatus,
  Field,
  ProcessorKind,
  ProcessorReading,
  ReadingOf,
  ServerEvent,
} from "./types";

export interface ProcessorState {
  reading: ProcessorReading;
  status?: ChannelStatus;
  receivedAt: number;
}

export interface ProcessorStore {
  byNode: Readonly<Record<string, ProcessorState>>;
  observe: (event: ServerEvent) => void;
  forget: (nodes: readonly string[]) => void;
  reset: () => void;
}

export const STALE_FLOOR_MS = 2_000;

export const useProcessorStore = create<ProcessorStore>((set) => ({
  byNode: {},
  observe: (event) => {
    if (event.type !== "ProcessorUpdate") {
      return;
    }
    const { node, reading, status } = event.data;
    set((state) => ({
      byNode: { ...state.byNode, [node]: { reading, status, receivedAt: Date.now() } },
    }));
  },
  forget: (nodes) => set((state) => ({ byNode: omitNodes(state.byNode, nodes) })),
  reset: () => set({ byNode: {} }),
}));

export function readingOf<T extends ProcessorKind>(
  state: ProcessorState | undefined,
  type: T,
): ReadingOf<T> | null {
  if (state === undefined || state.reading.type !== type) {
    return null;
  }
  return state.reading.reading as ReadingOf<T>;
}

export function isStale(receivedAt: number, now: number, periodMs: number): boolean {
  return now - receivedAt > Math.max(3 * periodMs, STALE_FLOOR_MS);
}

const NO_READOUTS: readonly Field[] = [];

export function readoutsOf(state: ProcessorState | undefined): readonly Field[] {
  return state?.status?.readouts ?? NO_READOUTS;
}
