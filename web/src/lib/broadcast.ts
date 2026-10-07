import { create } from "zustand";
import { decoderKey } from "./hunt";
import type { BroadcastStatus, ServerEvent } from "./types";

export interface ChannelBroadcast {
  deviceSet: number;
  channel: number;
  status: BroadcastStatus;
}

interface BroadcastStore {
  byChannel: Readonly<Record<string, ChannelBroadcast>>;
  observe: (event: ServerEvent) => void;
  reset: () => void;
}

export const useBroadcastStore = create<BroadcastStore>((set) => ({
  byChannel: {},
  observe: (event) => {
    if (event.type !== "BroadcastUpdate") {
      return;
    }
    const { device_set: deviceSet, channel, status } = event.data;
    set((state) => ({
      byChannel: {
        ...state.byChannel,
        [decoderKey(deviceSet, channel)]: { deviceSet, channel, status },
      },
    }));
  },
  reset: () => set({ byChannel: {} }),
}));

export function useBroadcast(deviceSet: number | undefined, channel: number | undefined) {
  return useBroadcastStore((store) =>
    deviceSet === undefined || channel === undefined
      ? undefined
      : store.byChannel[decoderKey(deviceSet, channel)],
  );
}
