import type {
  AntennaReport,
  AntennaSimulation,
  AntennaSweepPoint,
  NanoVnaComplex,
  NanoVnaPoint,
} from "../../lib/types";
import type { HamBand } from "../nanovna/bands";
import { inputImpedance, lNetworks } from "../nanovna/matching";
import { formatSi, REFERENCE_OHMS } from "../nanovna/nanovna";

export function simulationOf(report: AntennaReport | null): AntennaSimulation | null {
  const outcome = report?.simulation;
  return outcome?.status === "solved" ? outcome : null;
}

export function unavailableReason(report: AntennaReport | null): string | null {
  const outcome = report?.simulation;
  return outcome?.status === "unavailable" ? outcome.reason : null;
}

export type Transform = (z: NanoVnaComplex, frequencyHz: number) => NanoVnaComplex;

const DIRECT: Transform = (z) => z;

export function reflection(z: NanoVnaComplex): NanoVnaComplex {
  const re = z.re + REFERENCE_OHMS;
  const im = z.im;
  const scale = re * re + im * im;
  if (scale === 0) {
    return { re: 1, im: 0 };
  }
  const nr = z.re - REFERENCE_OHMS;
  return { re: (nr * re + im * im) / scale, im: (im * re - nr * im) / scale };
}

export function swrOf(z: NanoVnaComplex): number {
  const gamma = reflection(z);
  const size = Math.hypot(gamma.re, gamma.im);
  return size >= 1 ? Number.POSITIVE_INFINITY : (1 + size) / (1 - size);
}

export function pointsOf(
  sweep: readonly AntennaSweepPoint[],
  transform: Transform = DIRECT,
): NanoVnaPoint[] {
  return sweep.map((point) => ({
    frequency_hz: Math.round(point.frequency_hz),
    s11: reflection(transform(point.impedance, point.frequency_hz)),
    s21: { re: 0, im: 0 },
  }));
}

export interface Matching {
  id: string;
  label: string;
  detail: string;
  apply: Transform;
}

const TRANSFORMER_RATIOS = [4, 9, 49] as const;

export function matchings(z: NanoVnaComplex, centerHz: number, balanced: boolean): Matching[] {
  const networks = lNetworks(z, centerHz);
  const transformer = balanced ? "balun" : "unun";
  return [
    { id: "direct", label: "Direct", detail: "Coax straight to the feedpoint", apply: DIRECT },
    ...TRANSFORMER_RATIOS.map((ratio) => ({
      id: `ratio-${ratio}`,
      label: `${ratio}:1 ${transformer}`,
      detail: `Divides the impedance by ${ratio}`,
      apply: (load: NanoVnaComplex) => ({ re: load.re / ratio, im: load.im / ratio }),
    })),
    ...quarterWave(z, centerHz),
    ...networks.map((network, index) => {
      const series = formatSi(
        network.series.value,
        network.series.kind === "inductor" ? "H" : "F",
        2,
      );
      const shunt = formatSi(network.shunt.value, network.shunt.kind === "inductor" ? "H" : "F", 2);
      const pass = network.series.kind === "inductor" ? "low" : "high";
      const twin = networks.some(
        (other, at) => at < index && other.series.kind === network.series.kind,
      );
      return {
        id: `l-${index}`,
        label: `L network, ${pass} pass${twin ? " 2" : ""}`,
        detail:
          network.shuntAt === "load"
            ? `Shunt ${shunt} at the antenna, series ${series}`
            : `Series ${series}, shunt ${shunt} at the radio`,
        apply: (load: NanoVnaComplex, hz: number) => inputImpedance(network, load, hz),
      };
    }),
  ];
}

function quarterWave(z: NanoVnaComplex, centerHz: number): Matching[] {
  const line = Math.sqrt(REFERENCE_OHMS * z.re);
  if (z.re <= 0 || Math.abs(z.im) > z.re * 0.25 || Math.abs(line - REFERENCE_OHMS) < 5) {
    return [];
  }
  return [
    {
      id: "quarter-wave",
      label: "λ/4 line",
      detail: `${Math.round(line)} Ω line, a quarter wave long`,
      apply: (load, hz) => throughLine(load, line, (Math.PI / 2) * (hz / centerHz)),
    },
  ];
}

export function throughLine(load: NanoVnaComplex, lineOhms: number, angle: number): NanoVnaComplex {
  const t = Math.tan(angle);
  const numerator = { re: load.re, im: load.im + lineOhms * t };
  const denominator = { re: lineOhms - load.im * t, im: load.re * t };
  const scale = denominator.re ** 2 + denominator.im ** 2;
  return {
    re: (lineOhms * (numerator.re * denominator.re + numerator.im * denominator.im)) / scale,
    im: (lineOhms * (numerator.im * denominator.re - numerator.re * denominator.im)) / scale,
  };
}

const GOOD_ENOUGH_SWR = 2;

export function bestMatching(
  list: readonly Matching[],
  z: NanoVnaComplex,
  centerHz: number,
): Matching | null {
  const simplest = list.find((matching) => swrOf(matching.apply(z, centerHz)) <= GOOD_ENOUGH_SWR);
  if (simplest !== undefined) {
    return simplest;
  }
  let best: Matching | null = null;
  let lowest = Number.POSITIVE_INFINITY;
  for (const matching of list) {
    const swr = swrOf(matching.apply(z, centerHz));
    if (swr < lowest - 0.01) {
      best = matching;
      lowest = swr;
    }
  }
  return best;
}

export interface Span {
  lowHz: number;
  highHz: number;
  openLow: boolean;
  openHigh: boolean;
}

export function swrSpan(
  sweep: readonly AntennaSweepPoint[],
  transform: Transform,
  centerHz: number,
  limit = 2,
): Span | null {
  const swr = sweep.map((point) => swrOf(transform(point.impedance, point.frequency_hz)));
  const start = nearest(sweep, centerHz);
  if (start < 0 || (swr[start] ?? Number.POSITIVE_INFINITY) > limit) {
    return null;
  }
  let low = start;
  while (low > 0 && (swr[low - 1] ?? Number.POSITIVE_INFINITY) <= limit) {
    low -= 1;
  }
  let high = start;
  while (high < sweep.length - 1 && (swr[high + 1] ?? Number.POSITIVE_INFINITY) <= limit) {
    high += 1;
  }
  return {
    lowHz: edge(sweep, swr, low, low - 1, limit),
    highHz: edge(sweep, swr, high, high + 1, limit),
    openLow: low === 0,
    openHigh: high === sweep.length - 1,
  };
}

function edge(
  sweep: readonly AntennaSweepPoint[],
  swr: readonly number[],
  inside: number,
  outside: number,
  limit: number,
): number {
  const a = sweep[inside];
  const b = sweep[outside];
  const sa = swr[inside];
  const sb = swr[outside];
  if (a === undefined || b === undefined || sa === undefined || sb === undefined) {
    return a?.frequency_hz ?? 0;
  }
  if (!Number.isFinite(sb)) {
    return a.frequency_hz;
  }
  const fraction = (limit - sa) / (sb - sa);
  return a.frequency_hz + fraction * (b.frequency_hz - a.frequency_hz);
}

function nearest(sweep: readonly AntennaSweepPoint[], hz: number): number {
  let best = -1;
  let distance = Number.POSITIVE_INFINITY;
  sweep.forEach((point, index) => {
    const gap = Math.abs(point.frequency_hz - hz);
    if (gap < distance) {
      best = index;
      distance = gap;
    }
  });
  return best;
}

export type ResonanceKind = "series" | "parallel";

export interface Resonance {
  frequencyHz: number;
  kind: ResonanceKind;
  resistanceOhms: number;
}

export function resonances(sweep: readonly AntennaSweepPoint[]): Resonance[] {
  const found: Resonance[] = [];
  for (let index = 1; index < sweep.length; index += 1) {
    const before = sweep[index - 1];
    const after = sweep[index];
    if (before === undefined || after === undefined) {
      continue;
    }
    const x0 = before.impedance.im;
    const x1 = after.impedance.im;
    if (x0 === 0 || Math.sign(x0) === Math.sign(x1)) {
      continue;
    }
    const fraction = x0 / (x0 - x1);
    found.push({
      frequencyHz: before.frequency_hz + fraction * (after.frequency_hz - before.frequency_hz),
      kind: x1 > x0 ? "series" : "parallel",
      resistanceOhms: before.impedance.re + fraction * (after.impedance.re - before.impedance.re),
    });
  }
  return found;
}

const MARGIN = 0.05;
const BAND_MARGIN = 0.5;

export interface SweepWindow {
  startHz: number;
  stopHz: number;
  points: number;
}

export function sweepWindow(
  centerHz: number,
  bands: readonly HamBand[],
  points: number,
): SweepWindow {
  const band = bands.find((entry) => entry.startHz <= centerHz && entry.stopHz >= centerHz);
  const low = centerHz * (1 - MARGIN);
  const high = centerHz * (1 + MARGIN);
  if (band === undefined) {
    return grid(low, high, points);
  }
  const pad = (band.stopHz - band.startHz) * BAND_MARGIN;
  return grid(Math.min(low, band.startHz - pad), Math.max(high, band.stopHz + pad), points);
}

export const WIDE_LOW = 0.4;
export const WIDE_HIGH = 3.2;

export function wideWindow(centerHz: number, points: number): SweepWindow {
  return grid(centerHz * WIDE_LOW, centerHz * WIDE_HIGH, points);
}

const NICE_STEPS = [1, 1.25, 1.5, 2, 2.5, 3, 4, 5, 6, 8, 10];

function grid(lowHz: number, highHz: number, points: number): SweepWindow {
  const raw = (highHz - lowHz) / Math.max(1, points - 1);
  const decade = 10 ** Math.floor(Math.log10(raw));
  const step = (NICE_STEPS.find((nice) => nice * decade >= raw * (1 - 1e-9)) ?? 10) * decade;
  const startHz = Math.ceil(lowHz / step - 1e-9) * step;
  const count = Math.floor((highHz - startHz) / step + 1e-9) + 1;
  return { startHz, stopHz: startHz + step * (count - 1), points: count };
}

export function roundedHz(hz: number): number {
  if (!Number.isFinite(hz) || hz <= 0) {
    return hz;
  }
  const decade = 10 ** (Math.floor(Math.log10(hz)) - 3);
  return Math.round(hz / decade) * decade;
}
