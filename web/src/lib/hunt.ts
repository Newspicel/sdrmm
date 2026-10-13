import { create } from "zustand";
import type { HuntStatus, ServerEvent } from "./types";

export const FLUSH_MS = 40;

export interface HuntState {
  byNode: Readonly<Record<string, HuntStatus>>;
  observe: (event: ServerEvent) => void;
  clear: (node: string) => void;
  reset: () => void;
}

let pending: Record<string, HuntStatus> | null = null;
let timer: ReturnType<typeof setTimeout> | null = null;

export const useHuntStore = create<HuntState>((set) => {
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
      if (event.type !== "HuntUpdate") {
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
