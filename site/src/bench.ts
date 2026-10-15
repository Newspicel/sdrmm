import apps from "./data/bench/apps.json";
import decoders from "./data/bench/decoders.json";
import dsp from "./data/bench/dsp.json";

export const SELF = "SDR--";

export interface Entry {
  tool: string;
  version: string;
  value: number;
}

export interface Group {
  id: string;
  subject: string;
  title: string;
  unit: string;
  better: "higher" | "lower";
  note?: string;
  results: Entry[];
}

export interface Machine {
  cpu: string;
  os: string;
  date: string;
}

export interface Suite {
  machine: Machine;
  groups: Group[];
}

export type Form = "rows" | "strips";

export type Span = "row" | "table";

export interface Section {
  id: string;
  title: string;
  command: string;
  form: Form;
  span: Span;
  suite: Suite;
}

export type Kind = "share" | "count" | "rate";

export interface Scale {
  log: boolean;
  min: number;
  max: number;
  ticks: number[];
}

export interface Table {
  unit: string;
  better: Group["better"];
  tools: string[];
  rows: Group[];
}

export const SECTIONS: Section[] = [
  {
    id: "dsp",
    title: "DSP kernels",
    command: "cargo xtask compare dsp",
    form: "rows",
    span: "row",
    suite: dsp as Suite,
  },
  {
    id: "decoders",
    title: "Decoders",
    command: "cargo xtask compare decoders",
    form: "strips",
    span: "row",
    suite: decoders as Suite,
  },
  {
    id: "apps",
    title: "Whole apps",
    command: "cargo xtask compare apps",
    form: "rows",
    span: "table",
    suite: apps as Suite,
  },
];

export function ranked(group: Group): Entry[] {
  const sign = group.better === "higher" ? -1 : 1;
  return group.results.toSorted(
    (a, b) => sign * (a.value - b.value) || Number(ours(b)) - Number(ours(a)),
  );
}

export function width(entry: Entry, group: Group): number {
  const top = Math.max(...group.results.map((result) => result.value));
  return top > 0 ? entry.value / top : 0;
}

export function amount(value: number): string {
  if (value >= 100 || Number.isInteger(value)) return Math.round(value).toLocaleString("en-US");
  if (value >= 10) return value.toFixed(1);
  return value.toFixed(2);
}

export function machine(suite: Suite): string {
  return [suite.machine.cpu, suite.machine.os, suite.machine.date].filter(Boolean).join(" · ");
}

export function measured(sections: readonly Section[]): Section[] {
  return sections.filter((section) => section.suite.groups.length > 0);
}

export function ours(entry: Entry): boolean {
  return entry.tool.startsWith(SELF);
}

export function lead(group: Group): number {
  const mine = group.results.filter(ours).map((entry) => entry.value);
  const others = group.results.filter((entry) => !ours(entry)).map((entry) => entry.value);
  if (mine.length === 0 || others.length === 0) return 0;
  const higher = group.better === "higher";
  const best = higher ? Math.max(...others) : Math.min(...others);
  const weakest = higher ? Math.min(...mine) : Math.max(...mine);
  const ratio = higher ? weakest / best : best / weakest;
  return Number.isFinite(ratio) ? ratio : 0;
}

export function highlights(sections: readonly Section[]): Group[] {
  return measured(sections).flatMap((section) => {
    const top = section.suite.groups.toSorted((a, b) => lead(b) - lead(a))[0];
    return top !== undefined && lead(top) > 1 ? [top] : [];
  });
}

function unique(values: readonly string[]): string[] {
  return [...new Set(values)];
}

export function tools(groups: readonly Group[]): string[] {
  const names = unique(groups.flatMap((group) => group.results.map((entry) => entry.tool)));
  return [
    ...names.filter((name) => name.startsWith(SELF)),
    ...names.filter((name) => !name.startsWith(SELF)),
  ];
}

export function subjects(groups: readonly Group[]): string[] {
  return unique(groups.map((group) => group.subject));
}

export function units(groups: readonly Group[]): string[] {
  return unique(groups.map((group) => group.unit));
}

export function tables(groups: readonly Group[]): Table[] {
  return units(groups).flatMap((unit) => {
    const rows = groups.filter((group) => group.unit === unit);
    const first = rows[0];
    return first === undefined ? [] : [{ unit, better: first.better, tools: tools(rows), rows }];
  });
}

const ORDER: Kind[] = ["share", "rate", "count"];

export function metrics(groups: readonly Group[]): string[] {
  return units(groups).toSorted(
    (a, b) => ORDER.indexOf(kind(a, groups)) - ORDER.indexOf(kind(b, groups)),
  );
}

export function find(groups: readonly Group[], subject: string, unit: string): Group | undefined {
  return groups.find((group) => group.subject === subject && group.unit === unit);
}

export function result(group: Group, tool: string): Entry | undefined {
  return group.results.find((entry) => entry.tool === tool);
}

export function peak(groups: readonly Group[]): number {
  return Math.max(0, ...groups.flatMap((group) => group.results.map((entry) => entry.value)));
}

export function best(group: Group): Entry | undefined {
  return ranked(group).find((entry) => !ours(entry));
}

export function mine(group: Group): Entry | undefined {
  return group.results.find(ours);
}

export function single(groups: readonly Group[]): boolean {
  return groups.every((group) => group.results.filter(ours).length === 1);
}

export function kind(unit: string, groups: readonly Group[]): Kind {
  if (unit.startsWith("%")) return "share";
  const values = groups
    .filter((group) => group.unit === unit)
    .flatMap((group) => group.results.map((entry) => entry.value));
  return values.every(Number.isInteger) ? "count" : "rate";
}

function factor(ratio: number): string {
  return ratio >= 10 ? Math.round(ratio).toString() : ratio.toFixed(1);
}

export function margin(group: Group, measure: Kind): string {
  const own = mine(group);
  const rival = best(group);
  if (own === undefined || rival === undefined) return "";
  const sign = group.better === "higher" ? 1 : -1;
  if (measure === "share") {
    const points = sign * (own.value - rival.value);
    if (Math.abs(points) < 0.05) return "even";
    return `${points > 0 ? "+" : "\u2212"}${Math.abs(points).toFixed(1)} pts`;
  }
  if (measure === "count") {
    const more = sign * (own.value - rival.value);
    if (more === 0) return "even";
    return `${more > 0 ? "+" : "\u2212"}${amount(Math.abs(more))}`;
  }
  const ratio = lead(group);
  if (ratio === 0) return "";
  const shown = factor(ratio >= 1 ? ratio : 1 / ratio);
  if (shown === "1.0") return "even";
  const [up, down] = group.better === "higher" ? ["faster", "slower"] : ["less", "more"];
  return `${shown}\u00d7 ${ratio > 1 ? up : down}`;
}

export function label(group: Group): string {
  const rest = group.title.replace(group.subject, "").replace(/^[,\s]+/, "");
  return rest.charAt(0).toUpperCase() + rest.slice(1);
}

function decades(values: readonly number[]): number[] {
  const positive = values.filter((value) => value > 0);
  if (positive.length === 0) return [1, 10];
  const low = Math.floor(Math.log10(Math.min(...positive)));
  const high = Math.max(low + 1, Math.ceil(Math.log10(Math.max(...positive))));
  return Array.from({ length: high - low + 1 }, (_, index) => 10 ** (low + index));
}

export function scale(unit: string, groups: readonly Group[]): Scale {
  if (kind(unit, groups) === "share") return { log: false, min: 0, max: 100, ticks: [0, 50, 100] };
  const values = groups
    .filter((group) => group.unit === unit)
    .flatMap((group) => group.results.map((entry) => entry.value));
  const ticks = decades(values);
  return { log: true, min: ticks[0] ?? 1, max: ticks.at(-1) ?? 10, ticks };
}

export function position(value: number, axis: Scale): number {
  const at = (x: number) => (axis.log ? Math.log10(Math.max(x, axis.min)) : x);
  const span = at(axis.max) - at(axis.min);
  if (span <= 0) return 0;
  return Math.min(1, Math.max(0, (at(value) - at(axis.min)) / span));
}

export function tick(value: number): string {
  if (value >= 1_000_000) return `${value / 1_000_000}M`;
  if (value >= 1000) return `${value / 1000}k`;
  return value.toString();
}

export function plotted(group: Group): Entry[] {
  return group.results.toSorted((a, b) => Number(ours(a)) - Number(ours(b)));
}
