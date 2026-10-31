import { create } from "zustand";
import type { ScheduleStatus, ServerEvent } from "./types";

interface ScheduleStore {
  byNode: Readonly<Record<string, ScheduleStatus>>;
  observe: (event: ServerEvent) => void;
  reset: () => void;
}

export const useScheduleStore = create<ScheduleStore>((set) => ({
  byNode: {},
  observe: (event) => {
    if (event.type !== "ScheduleUpdate") {
      return;
    }
    const status = event.data.status;
    set((state) => ({ byNode: { ...state.byNode, [status.node]: status } }));
  },
  reset: () => set({ byNode: {} }),
}));
