import type { PointReadout } from "./analysis";
import { formatSi } from "./nanovna";
import type { Domain, PlotGuide, PlotTrace } from "./Plot";

export type ViewId = "swr" | "s11" | "impedance" | "phase" | "s21" | "delay" | "tdr";

export interface Subject {
  key: string;
  label: string;
  color: string;
  dashed: boolean;
  rows: readonly PointReadout[];
}

interface Quantity {
  key: string;
  label: string;
  color?: string;
  valueOf: (row: PointReadout) => number;
}

export interface View {
  id: ViewId;
  label: string;
  title: string;
  quantities: Quantity[];
  domain: (values: number[]) => Domain;
  guides: PlotGuide[];
  format: (value: number) => string;
}

export const LIVE_COLOR = "var(--color-plot-trace)";
const SECOND_COLOR = "var(--color-plot-hold)";

export const REFERENCE_COLORS = [
  "var(--color-plot-ref-1)",
  "var(--color-plot-ref-2)",
  "var(--color-plot-ref-3)",
  "var(--color-plot-ref-4)",
] as const;

const SWR_CEILING = 10;
const SWR_LIMIT = 1000;

export const VIEWS: readonly View[] = [
  {
    id: "swr",
    label: "SWR",
    title: "Standing wave ratio",
    quantities: [{ key: "swr", label: "SWR", valueOf: (row) => row.vswr }],
    domain: swrDomain,
    guides: [
      { value: 1.5, label: "1.5" },
      { value: 2, label: "2:1" },
      { value: 3, label: "3:1" },
    ],
    format: (value) => value.toFixed(2),
  },
  {
    id: "s11",
    label: "Return loss",
    title: "|S11| in dB",
    quantities: [{ key: "s11", label: "|S11|", valueOf: (row) => row.s11Db }],
    domain: (values) => ({ low: Math.min(-10, snapDown(robustLow(values), 10)), high: 0 }),
    guides: [
      { value: -9.54, label: "2:1" },
      { value: -14, label: "1.5" },
    ],
    format: (value) => `${value.toFixed(2)} dB`,
  },
  {
    id: "impedance",
    label: "Impedance",
    title: "Resistance and reactance",
    quantities: [
      {
        key: "r",
        label: "R",
        color: LIVE_COLOR,
        valueOf: (row) => row.impedance?.re ?? Number.NaN,
      },
      {
        key: "x",
        label: "X",
        color: SECOND_COLOR,
        valueOf: (row) => row.impedance?.im ?? Number.NaN,
      },
    ],
    domain: (values) => robustDomain([...values, 0], 25),
    guides: [{ value: 50, label: "50 Ω" }],
    format: (value) => `${value.toFixed(1)} Ω`,
  },
  {
    id: "phase",
    label: "Phase",
    title: "S11 phase",
    quantities: [{ key: "phase", label: "S11 phase", valueOf: (row) => row.s11PhaseDeg }],
    domain: () => ({ low: -180, high: 180 }),
    guides: [],
    format: (value) => `${value.toFixed(1)}°`,
  },
  {
    id: "s21",
    label: "Through",
    title: "|S21| in dB",
    quantities: [{ key: "s21", label: "|S21|", valueOf: (row) => row.s21Db }],
    domain: (values) => robustDomain(values, 10),
    guides: [],
    format: (value) => `${value.toFixed(2)} dB`,
  },
  {
    id: "delay",
    label: "Delay",
    title: "S21 group delay",
    quantities: [{ key: "delay", label: "Delay", valueOf: (row) => row.groupDelayS }],
    domain: (values) => robustDomain(values, 0),
    guides: [],
    format: (value) => formatSi(value, "s", 2),
  },
];

export function viewFor(id: ViewId): View {
  return VIEWS.find((view) => view.id === id) ?? (VIEWS[0] as View);
}

export function tracesFor(view: View, subjects: readonly Subject[]): PlotTrace[] {
  if (view.quantities.length > 1) {
    const live = subjects[0];
    return live === undefined
      ? []
      : view.quantities.map((quantity) => ({
          key: `${live.key}:${quantity.key}`,
          label: quantity.label,
          color: quantity.color ?? live.color,
          dashed: false,
          xs: live.rows.map((row) => row.frequencyHz),
          values: live.rows.map(quantity.valueOf),
        }));
  }
  const quantity = view.quantities[0];
  if (quantity === undefined) {
    return [];
  }
  return subjects.map((subject) => ({
    key: `${subject.key}:${quantity.key}`,
    label: subject.label,
    color: subject.color,
    dashed: subject.dashed,
    xs: subject.rows.map((row) => row.frequencyHz),
    values: subject.rows.map(quantity.valueOf),
  }));
}

export function visibleValues(traces: readonly PlotTrace[], window: Domain): number[] {
  return traces.flatMap((trace) =>
    trace.values.filter((_, index) => {
      const x = trace.xs[index];
      return x !== undefined && x >= window.low && x <= window.high;
    }),
  );
}

export function nearestIndex(xs: readonly number[], x: number): number {
  let low = 0;
  let high = xs.length - 1;
  while (high - low > 1) {
    const middle = (low + high) >> 1;
    if ((xs[middle] ?? 0) < x) {
      low = middle;
    } else {
      high = middle;
    }
  }
  const below = xs[low] ?? 0;
  const above = xs[high] ?? 0;
  return Math.abs(x - below) <= Math.abs(above - x) ? low : high;
}

export function swrDomain(values: number[]): Domain {
  const kept = finite(values);
  const lowest = kept.length === 0 ? 1 : Math.min(...kept);
  const highest = finiteMax(values, 3);
  if (highest <= SWR_CEILING || lowest < SWR_CEILING / 2) {
    return { low: 1, high: Math.min(SWR_CEILING, Math.max(3, Math.ceil(highest))) };
  }
  const decade = 10 ** Math.ceil(Math.log10(Math.min(highest, SWR_LIMIT)));
  return { low: 1, high: decade };
}

function finite(values: number[]): number[] {
  return values.filter(Number.isFinite);
}

function finiteMax(values: number[], fallback: number): number {
  const kept = finite(values);
  return kept.length === 0 ? fallback : Math.max(...kept);
}

function quantile(sorted: number[], fraction: number): number {
  return sorted[Math.min(sorted.length - 1, Math.floor(fraction * (sorted.length - 1)))] ?? 0;
}

function robustLow(values: number[]): number {
  const kept = finite(values).toSorted((left, right) => left - right);
  return kept.length === 0 ? -10 : quantile(kept, 0.01);
}

function snapDown(value: number, step: number): number {
  return Math.floor(value / step) * step;
}

function robustDomain(values: number[], minimumSpan: number): Domain {
  const kept = finite(values).toSorted((left, right) => left - right);
  if (kept.length === 0) {
    return { low: -1, high: 1 };
  }
  const low = quantile(kept, 0.02);
  const high = quantile(kept, 0.98);
  const margin = Math.max((high - low) * 0.08, (minimumSpan - (high - low)) / 2, 0);
  const pad = margin === 0 ? Math.abs(high) * 0.1 || 1 : margin;
  return { low: low - pad, high: high + pad };
}
