import data from "./data/modes.json";

export interface Band {
  low: number;
  high: number;
}

export interface Mode {
  type_id: string;
  name: string;
  summary: string;
  family: string;
  verification: string;
  input_rate_hz: number;
  bands: Band[];
}

export interface Level {
  id: string;
  label: string;
  means: string;
}

export interface Family {
  id: string;
  title: string;
}

export interface Catalog {
  levels: Level[];
  families: Family[];
  modes: Mode[];
}

export const CATALOG: Catalog = data;

export const ALL = "all";

const PREFIXES: [number, string][] = [
  [1e9, "G"],
  [1e6, "M"],
  [1e3, "k"],
  [1, ""],
];

function scaleOf(value: number): [number, string] {
  return PREFIXES.find(([scale]) => Math.abs(value) >= scale) ?? [1, ""];
}

function number(value: number): string {
  return String(Number(value.toFixed(4)));
}

export function si(value: number, unit: string): string {
  const [scale, prefix] = scaleOf(value);
  return `${number(value / scale)} ${prefix}${unit}`;
}

export function rate(hz: number): string {
  return si(hz, "S/s");
}

/** The span every allocation falls in, in the unit of its top edge; empty for anywhere. */
export function band(bands: readonly Band[]): string {
  if (bands.length === 0) {
    return "";
  }
  const low = Math.min(...bands.map((range) => range.low));
  const high = Math.max(...bands.map((range) => range.high));
  const [scale, prefix] = scaleOf(high);
  return `${number(low / scale)} to ${number(high / scale)} ${prefix}Hz`;
}

export function allocations(bands: readonly Band[]): string {
  return bands.map((range) => `${si(range.low, "Hz")} to ${si(range.high, "Hz")}`).join(", ");
}

export function matches(mode: Mode, query: string): boolean {
  const wanted = query.trim().toLowerCase();
  return wanted === "" || mode.name.toLowerCase().includes(wanted) || mode.type_id.includes(wanted);
}

export function shown(mode: Mode, query: string, family: string): boolean {
  return (family === ALL || mode.family === family) && matches(mode, query);
}

/** How many modes match the query, in all and per family. */
export function counts(catalog: Catalog, query: string): Map<string, number> {
  const found = catalog.modes.filter((mode) => matches(mode, query));
  const tally = new Map<string, number>([[ALL, found.length]]);
  for (const family of catalog.families) {
    tally.set(family.id, found.filter((mode) => mode.family === family.id).length);
  }
  return tally;
}

export function levelOf(catalog: Catalog, id: string): Level | undefined {
  return catalog.levels.find((level) => level.id === id);
}

export function familyTitle(catalog: Catalog, id: string): string {
  return catalog.families.find((family) => family.id === id)?.title ?? id;
}
