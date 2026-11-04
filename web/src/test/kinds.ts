import type { Capability, EventKindEntry, PatchCatalog } from "../lib/types";
import { CATALOG } from "./catalog";

function probe(kind: string, name: string, noun: string, caps: Capability[]): EventKindEntry {
  return { kind, name, noun, caps, hue: "teal", lifetime: { type: "instant" } };
}

/** Made-up decoder kinds, so tests never lean on a real decoder's registration. */
export const PROBE_KINDS: readonly EventKindEntry[] = [
  probe("probe_plane", "Probe plane", "Aircraft", ["log", "station", "geo", "track"]),
  probe("probe_link", "Probe link", "Aircraft", ["log", "station"]),
  probe("probe_ship", "Probe ship", "Vessels", ["log", "station", "geo"]),
  probe("probe_pager", "Probe pager", "Pagers", ["log"]),
  probe("probe_text", "Probe text", "Senders", ["log", "text"]),
];

export const PROBE_CATALOG: PatchCatalog = {
  ...CATALOG,
  event_kinds: [...CATALOG.event_kinds, ...PROBE_KINDS],
};
