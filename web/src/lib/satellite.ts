import { create } from "zustand";
import { eventSourcesOf } from "../canvas/binding";
import { nodeOf } from "../canvas/graph";
import type { PatchGraph, SatelliteStatus, ServerEvent } from "./types";

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

export function trackedBy(
  statuses: Readonly<Record<string, SatelliteStatus>>,
  channelNode: string,
): SatelliteStatus | null {
  return (
    Object.values(statuses).find((status) => status.driving?.includes(channelNode) ?? false) ?? null
  );
}

export function satelliteSourcesOf(graph: PatchGraph, map: string): string[] {
  return eventSourcesOf(graph, map).filter((id) => nodeOf(graph, id)?.kind === "satellite");
}
