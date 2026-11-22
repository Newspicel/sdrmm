import { create } from "zustand";
import type { Readout, ServerEvent } from "./types";

const NONE: readonly Readout[] = [];

export const useReadouts = create<{
  byDevice: ReadonlyMap<number, readonly Readout[]>;
  observe: (event: ServerEvent) => void;
  reset: () => void;
}>((set) => ({
  byDevice: new Map(),
  observe: (event) => {
    if (event.type !== "DeviceReadouts") return;
    const { device_set, readouts } = event.data;
    set((state) => ({ byDevice: new Map(state.byDevice).set(device_set, readouts) }));
  },
  reset: () => set({ byDevice: new Map() }),
}));

export function useDeviceReadouts(deviceSet: number): readonly Readout[] {
  return useReadouts((state) => state.byDevice.get(deviceSet) ?? NONE);
}

export interface ReadoutStat {
  name: string;
  label: string;
  text: string;
  unit: string;
  warn: boolean;
}

export function readoutStats(readouts: readonly Readout[]): ReadoutStat[] {
  const stats: ReadoutStat[] = [];
  for (const readout of readouts) {
    const held = stats.find((stat) => stat.name === readout.name);
    if (readout.value.kind === "flag") {
      const flag = readout.value;
      stats.push({
        name: readout.name,
        label: readout.label,
        text: (flag.on ? flag.on_text : flag.off_text) ?? (flag.on ? "on" : "off"),
        unit: "",
        warn: flag.warn_when === flag.on,
      });
      continue;
    }
    const text = readout.value.value.toFixed(1);
    if (held === undefined) {
      stats.push({
        name: readout.name,
        label: readout.label,
        text,
        unit: readout.value.unit,
        warn: false,
      });
    } else {
      held.text = `${held.text} / ${text}`;
    }
  }
  return stats;
}
