import { create } from "zustand";
import type { BroadcastStatus, ServerEvent } from "./types";

interface BroadcastStore {
  byNode: Readonly<Record<string, BroadcastStatus>>;
  observe: (event: ServerEvent) => void;
  reset: () => void;
}

export const useBroadcastStore = create<BroadcastStore>((set) => ({
  byNode: {},
  observe: (event) => {
    if (event.type !== "ChannelBroadcast") {
      return;
    }
    const { node, status } = event.data;
    set((state) => ({ byNode: { ...state.byNode, [node]: status } }));
  },
  reset: () => set({ byNode: {} }),
}));

export function useBroadcast(node: string): BroadcastStatus | undefined {
  return useBroadcastStore((store) => store.byNode[node]);
}
