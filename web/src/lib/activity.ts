import { create } from "zustand";
import type { PortActivity, ServerEvent } from "./types";

interface ActivityStore {
  ports: Readonly<Record<string, PortActivity>>;
  observe: (event: ServerEvent) => void;
  reset: () => void;
}

function activityKey(node: string, port: string): string {
  return `${node}\u0000${port}`;
}

export const useActivityStore = create<ActivityStore>((set) => ({
  ports: {},
  observe: (event) => {
    if (event.type !== "NodeActivity") {
      return;
    }
    const ports: Record<string, PortActivity> = {};
    for (const port of event.data.ports) {
      ports[activityKey(port.node, port.port)] = port;
    }
    set({ ports });
  },
  reset: () => set({ ports: {} }),
}));

export function usePortActivity(node: string, port: string): PortActivity | undefined {
  return useActivityStore((store) => store.ports[activityKey(node, port)]);
}

export function activityText(activity: PortActivity | undefined): string | null {
  if (activity === undefined) {
    return null;
  }
  const counts: [number, string][] = [
    [activity.delivered, "delivered"],
    [activity.unrouted, "unrouted"],
    [activity.ignored, "ignored"],
    [activity.lost, "lost"],
  ];
  const parts = counts.filter(([count]) => count > 0).map(([count, what]) => `${count} ${what}`);
  return parts.length === 0 ? null : parts.join(", ");
}

export function idlePort(activity: PortActivity | undefined): boolean {
  return activity !== undefined && activity.emitted > 0 && activity.delivered === 0;
}
