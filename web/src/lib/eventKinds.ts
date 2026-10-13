import { useQuery } from "@tanstack/react-query";
import { patchCatalogQuery } from "./api";
import type { Capability, EventKind, EventKindEntry } from "./types";

const NONE: readonly EventKindEntry[] = [];

export function kindEntry(
  kind: string,
  entries: readonly EventKindEntry[],
): EventKindEntry | undefined {
  return entries.find((entry) => entry.kind === kind);
}

export function isEventKind(kind: string, entries: readonly EventKindEntry[]): kind is EventKind {
  return kindEntry(kind, entries) !== undefined;
}

export function kindHas(
  kind: string,
  cap: Capability,
  entries: readonly EventKindEntry[],
): boolean {
  return kindEntry(kind, entries)?.caps.includes(cap) ?? false;
}

export function kindLabel(kind: string, entries: readonly EventKindEntry[]): string {
  return kindEntry(kind, entries)?.name ?? kind.toUpperCase();
}

export function useEventKinds(): readonly EventKindEntry[] {
  return useQuery(patchCatalogQuery()).data?.event_kinds ?? NONE;
}

export function useKindLabel(): (kind: string) => string {
  const entries = useEventKinds();
  return (kind) => kindLabel(kind, entries);
}
