import { create } from "zustand";
import type { ChannelStatus, ParamChoice, ServerEvent } from "./types";

interface ChannelStatusStore {
  byNode: Readonly<Record<string, ChannelStatus>>;
  observe: (event: ServerEvent) => void;
  reset: () => void;
}

export const useChannelStatusStore = create<ChannelStatusStore>((set) => ({
  byNode: {},
  observe: (event) => {
    if (event.type !== "ChannelStatus") {
      return;
    }
    const { node, status } = event.data;
    set((state) => ({ byNode: { ...state.byNode, [node]: status } }));
  },
  reset: () => set({ byNode: {} }),
}));

export function useChannelStatus(node: string): ChannelStatus | undefined {
  return useChannelStatusStore((store) => store.byNode[node]);
}

const NO_CHOICES: readonly ParamChoice[] = [];

export function statusChoices(status: ChannelStatus | undefined): readonly ParamChoice[] {
  return status?.choices ?? NO_CHOICES;
}
