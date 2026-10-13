import type { DeviceSet, PatchGraph } from "../lib/types";
import { nodeOf } from "./graph";

export interface DeviceTarget {
  kind: "device";
  node: string;
  set: DeviceSet;
}

export interface ChannelTarget {
  kind: "channel";
  node: string;
}

export type TuneTarget = DeviceTarget | ChannelTarget;

export function libraryTarget(
  graph: PatchGraph,
  devices: ReadonlyMap<string, DeviceSet>,
  selected: string | null,
): TuneTarget | null {
  if (selected === null) {
    return null;
  }
  const set = devices.get(selected);
  if (set !== undefined) {
    return { kind: "device", node: selected, set };
  }
  return nodeOf(graph, selected)?.kind === "channel" ? { kind: "channel", node: selected } : null;
}
