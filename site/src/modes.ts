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

/** Whether a mode passes the search and the picked levels; no level picked means every level. */
export function shown(mode: Mode, query: string, levels: ReadonlySet<string>): boolean {
  return (levels.size === 0 || levels.has(mode.verification)) && matches(mode, query);
}

/** How many modes are shown, in all and per family. */
export function counts(
  catalog: Catalog,
  query: string,
  levels: ReadonlySet<string> = new Set(),
): Map<string, number> {
  const found = catalog.modes.filter((mode) => shown(mode, query, levels));
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

/** The ITU bands the strip spans, LF to UHF. */
export const RADIO_BANDS: readonly { name: string; low: number; high: number }[] = [
  { name: "LF", low: 30e3, high: 300e3 },
  { name: "MF", low: 300e3, high: 3e6 },
  { name: "HF", low: 3e6, high: 30e6 },
  { name: "VHF", low: 30e6, high: 300e6 },
  { name: "UHF", low: 300e6, high: 3e9 },
];

const SPAN_LOW = 30e3;
const SPAN_HIGH = 3e9;
/** The narrowest a bar is drawn, as a share of the strip, so a single channel still shows. */
const MIN_WIDTH = 0.006;
/** The space kept between two bars in one lane. */
const LANE_GAP = 0.004;

/** Where `hz` falls on the strip, 0 at its low edge and 1 at its high edge. */
export function position(hz: number): number {
  const at = Math.log10(hz / SPAN_LOW) / Math.log10(SPAN_HIGH / SPAN_LOW);
  return Math.min(1, Math.max(0, at));
}

export interface Bar {
  mode: Mode;
  band: Band;
  start: number;
  width: number;
  lane: number;
}

/** Every allocation as a bar, stacked into the fewest lanes that keep bars apart. */
export function bars(modes: readonly Mode[]): Bar[] {
  const placed = modes
    .flatMap((mode) => mode.bands.map((range) => ({ mode, band: range })))
    .map(({ mode, band: range }) => {
      const start = position(range.low);
      const width = Math.max(MIN_WIDTH, position(range.high) - start);
      return { mode, band: range, start, width, lane: 0 };
    })
    .toSorted((a, b) => a.start - b.start);
  const ends: number[] = [];
  for (const bar of placed) {
    const free = ends.findIndex((end) => end + LANE_GAP <= bar.start);
    bar.lane = free === -1 ? ends.length : free;
    ends[bar.lane] = bar.start + bar.width;
  }
  return placed;
}
