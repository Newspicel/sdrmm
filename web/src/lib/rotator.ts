import { create } from "zustand";
import type { RotatorStatus, ServerEvent } from "./types";

interface RotatorStore {
  byNode: Readonly<Record<string, RotatorStatus>>;
  observe: (event: ServerEvent) => void;
  reset: () => void;
}

export const useRotatorStore = create<RotatorStore>((set) => ({
  byNode: {},
  observe: (event) => {
    if (event.type !== "RotatorUpdate") {
      return;
    }
    const status = event.data.status;
    set((state) => ({ byNode: { ...state.byNode, [status.node]: status } }));
  },
  reset: () => set({ byNode: {} }),
}));
