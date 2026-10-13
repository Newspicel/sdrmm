import { create } from "zustand";
import type { ScannerStatus, ServerEvent } from "./types";

export const FLUSH_MS = 150;

export interface ScannerState {
  byNode: Readonly<Record<string, ScannerStatus>>;
  observe: (event: ServerEvent) => void;
  clear: (node: string) => void;
  reset: () => void;
}

let pending: Record<string, ScannerStatus> | null = null;
let timer: ReturnType<typeof setTimeout> | null = null;

export const useScannerStore = create<ScannerState>((set) => {
  const flush = () => {
    timer = null;
    const staged = pending;
    pending = null;
    if (staged === null) {
      return;
    }
    set((state) => ({ byNode: { ...state.byNode, ...staged } }));
  };

  return {
    byNode: {},
    observe: (event: ServerEvent) => {
      if (event.type !== "ScannerUpdate") {
        return;
      }
      const node = event.data.status.settings.node;
      if (node == null) {
        return;
      }
      pending = { ...pending, [node]: event.data.status };
      if (timer === null) {
        timer = setTimeout(flush, FLUSH_MS);
      }
    },
    clear: (key: string) => {
      if (pending !== null) {
        delete pending[key];
      }
      set((state) => {
        if (!(key in state.byNode)) {
          return state;
        }
        const { [key]: _dropped, ...rest } = state.byNode;
        return { byNode: rest };
      });
    },
    reset: () => {
      pending = null;
      if (timer !== null) {
        clearTimeout(timer);
        timer = null;
      }
      set({ byNode: {} });
    },
  };
});
