import { kindEntry, kindHas } from "../../lib/eventKinds";
import { MAX_FILTER_IDS, MAX_FILTER_TEXT_LEN } from "../../lib/limits";
import type {
  Capability,
  EventFilterNode,
  EventKindEntry,
  Feed,
  FilterMode,
} from "../../lib/types";

export type TriState = "any" | "yes" | "no";

export function parseIds(text: string): number[] {
  const seen = new Set<number>();
  for (const token of text.split(/[\s,]+/)) {
    if (token === "") {
      continue;
    }
    const value = Number(token);
    if (!Number.isInteger(value) || value < 0) {
      continue;
    }
    seen.add(value);
  }
  return [...seen].slice(0, MAX_FILTER_IDS);
}

export function formatIds(ids: readonly number[] | undefined): string {
  return (ids ?? []).join(", ");
}

export function textLength(text: string): number {
  return Array.from(text).length;
}

export function parseWords(text: string): string[] {
  const seen = new Set<string>();
  for (const token of text.split(/[\s,]+/)) {
    const word = token.trim();
    if (word !== "" && textLength(word) <= MAX_FILTER_TEXT_LEN) {
      seen.add(word);
    }
  }
  return [...seen].slice(0, MAX_FILTER_IDS);
}

export function formatWords(words: readonly string[] | undefined): string {
  return (words ?? []).join(", ");
}

function everyKindHas(
  kinds: readonly string[],
  cap: Capability,
  entries: readonly EventKindEntry[],
): boolean {
  return kinds.length > 0 && kinds.every((kind) => kindHas(kind, cap, entries));
}

export function stationLabel(kinds: readonly string[], entries: readonly EventKindEntry[]): string {
  if (everyKindHas(kinds, "voice", entries)) {
    return "Radios seen";
  }
  const nouns = new Set(kinds.map((kind) => kindEntry(kind, entries)?.noun));
  const [noun] = nouns;
  if (everyKindHas(kinds, "station", entries) && nouns.size === 1 && noun !== undefined) {
    return noun;
  }
  return "Stations";
}

export function toTriState(value: boolean | null | undefined): TriState {
  if (value == null) {
    return "any";
  }
  return value ? "yes" : "no";
}

export function fromTriState(state: TriState): boolean | undefined {
  if (state === "any") {
    return undefined;
  }
  return state === "yes";
}

export function offeredBy(feeds: readonly Feed[]): string[] {
  const kinds = new Set<string>();
  for (const feed of feeds) {
    if (feed.payload.type === "events") {
      for (const kind of feed.payload.kinds) kinds.add(kind);
    }
  }
  return [...kinds].toSorted();
}

export const FILTER_MODES: readonly { value: FilterMode; label: string; title: string }[] = [
  { value: "keep", label: "Keep", title: "Only events matching every rule pass" },
  { value: "drop", label: "Drop", title: "Events matching every rule are removed" },
];

export function filterMode(filter: EventFilterNode): FilterMode {
  return filter.mode ?? "keep";
}

export function filterSaid(filter: EventFilterNode): string {
  const parts: string[] = [];
  const kinds = filter.kinds ?? [];
  parts.push(kinds.length === 0 ? "every event" : kinds.join(", "));
  if ((filter.stations ?? []).length > 0) {
    parts.push(formatWords(filter.stations));
  }
  if ((filter.contains ?? "") !== "") {
    parts.push(`"${filter.contains}"`);
  }
  if (filter.has_position != null) {
    parts.push(filter.has_position ? "with a fix" : "without a fix");
  }
  if ((filter.talkgroups ?? []).length > 0) {
    parts.push(`TG ${formatIds(filter.talkgroups)}`);
  }
  if ((filter.radios ?? []).length > 0) {
    parts.push(`radio ${formatIds(filter.radios)}`);
  }
  if (filter.encrypted != null) {
    parts.push(filter.encrypted ? "encrypted" : "clear");
  }
  if (filter.emergency != null) {
    parts.push(filter.emergency ? "emergency" : "routine");
  }
  if ((filter.min_duration_ms ?? 0) > 0) {
    parts.push(`over ${((filter.min_duration_ms ?? 0) / 1000).toFixed(1)} s`);
  }
  if (filterMode(filter) === "drop" && parts.length === 1 && kinds.length === 0) {
    return "drop nothing";
  }
  return `${filterMode(filter)} ${parts.join(" · ")}`;
}

export type PredicateKey =
  | "stations"
  | "contains"
  | "has_position"
  | "talkgroups"
  | "radios"
  | "encrypted"
  | "emergency"
  | "min_duration_ms";

export function predicatesFor(
  kinds: readonly string[],
  entries: readonly EventKindEntry[],
): PredicateKey[] {
  const touches = (cap: Capability) =>
    kinds.length === 0 || kinds.some((kind) => kindHas(kind, cap, entries));
  const shown: PredicateKey[] = ["stations", "contains"];
  if (touches("geo")) {
    shown.push("has_position");
  }
  if (touches("voice")) {
    shown.push("talkgroups", "radios", "encrypted", "emergency");
  }
  if (touches("duration")) {
    shown.push("min_duration_ms");
  }
  return shown;
}

export interface PredicateSection {
  key: string;
  title: string;
  applies: string[];
  predicates: PredicateKey[];
}

export function sectionsFor(
  kinds: readonly string[],
  entries: readonly EventKindEntry[],
): PredicateSection[] {
  const shown = predicatesFor(kinds, entries);
  const pick = (keys: PredicateKey[]) => keys.filter((key) => shown.includes(key));
  const scope = (cap: Capability) => kinds.filter((kind) => kindHas(kind, cap, entries)).toSorted();
  const sections: PredicateSection[] = [];
  const any = pick(["stations", "contains"]);
  if (any.length > 0) {
    sections.push({
      key: "any",
      title: "Any event",
      applies: [...kinds].toSorted(),
      predicates: any,
    });
  }
  const position = pick(["has_position"]);
  if (position.length > 0) {
    sections.push({
      key: "position",
      title: "Position",
      applies: scope("geo"),
      predicates: position,
    });
  }
  const voice = pick(["talkgroups", "radios", "encrypted", "emergency", "min_duration_ms"]);
  if (voice.length > 0) {
    sections.push({
      key: "voice",
      title: "Voice",
      applies: scope("voice"),
      predicates: voice,
    });
  }
  return sections;
}
