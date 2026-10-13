import { create } from "zustand";
import type { SatelliteStatus, ServerEvent } from "./types";

interface SatelliteStore {
  byNode: Readonly<Record<string, SatelliteStatus>>;
  observe: (event: ServerEvent) => void;
  reset: () => void;
}

export const useSatelliteStore = create<SatelliteStore>((set) => ({
  byNode: {},
  observe: (event) => {
    if (event.type !== "SatelliteUpdate") {
      return;
    }
    const status = event.data.status;
    set((state) => ({ byNode: { ...state.byNode, [status.node]: status } }));
  },
  reset: () => set({ byNode: {} }),
}));
