import type { BandAllocation, BandPlan } from "../../lib/types";
import type { PointReadout } from "./analysis";

export interface HamBand {
  label: string;
  startHz: number;
  stopHz: number;
}

export interface BandFit {
  band: HamBand;
  bestVswr: number;
  bestHz: number;
  worstVswr: number;
  points: number;
}

interface Group {
  startHz: number;
  stopHz: number;
  allocations: BandAllocation[];
}

const WAVELENGTH_PREFIX = /^\d+(?:\.\d+)?\s?(?:m|cm|mm)(?![a-z])/i;

const FREQUENCY_UNITS = [
  { factor: 1e9, unit: "GHz" },
  { factor: 1e6, unit: "MHz" },
  { factor: 1e3, unit: "kHz" },
] as const;

export function hamBands(plan: BandPlan): HamBand[] {
  const amateur = plan.allocations
    .filter(
      (allocation) =>
        allocation.service === "amateur" &&
        Number.isFinite(allocation.start_hz) &&
        allocation.stop_hz > allocation.start_hz,
    )
    .toSorted((first, second) => first.start_hz - second.start_hz);
  return mergeAllocations(amateur).map((group) => ({
    label: wavelengthLabel(group.allocations) ?? frequencyLabel(group.startHz),
    startHz: group.startHz,
    stopHz: group.stopHz,
  }));
}

export function bandsWithin(bands: readonly HamBand[], lowHz: number, highHz: number): HamBand[] {
  return bands.filter((band) => band.startHz <= highHz && band.stopHz >= lowHz);
}

export function bandFits(rows: readonly PointReadout[], bands: readonly HamBand[]): BandFit[] {
  return bands.flatMap((band) => {
    const inside = rows.filter(
      (row) => row.frequencyHz >= band.startHz && row.frequencyHz <= band.stopHz,
    );
    return inside.length === 0 ? [] : [fitOf(band, inside)];
  });
}

function fitOf(band: HamBand, inside: readonly PointReadout[]): BandFit {
  let best: PointReadout | null = null;
  let worstVswr = Number.NEGATIVE_INFINITY;
  for (const row of inside) {
    if (Number.isFinite(row.vswr) && (best === null || row.vswr < best.vswr)) {
      best = row;
    }
    if (!Number.isNaN(row.vswr)) {
      worstVswr = Math.max(worstVswr, row.vswr);
    }
  }
  return {
    band,
    bestVswr: best?.vswr ?? Number.POSITIVE_INFINITY,
    bestHz: best?.frequencyHz ?? inside[0]?.frequencyHz ?? band.startHz,
    worstVswr: worstVswr === Number.NEGATIVE_INFINITY ? Number.NaN : worstVswr,
    points: inside.length,
  };
}

function mergeAllocations(sorted: readonly BandAllocation[]): Group[] {
  const groups: Group[] = [];
  for (const allocation of sorted) {
    const current = groups.at(-1);
    if (current !== undefined && allocation.start_hz <= current.stopHz) {
      current.stopHz = Math.max(current.stopHz, allocation.stop_hz);
      current.allocations.push(allocation);
    } else {
      groups.push({
        startHz: allocation.start_hz,
        stopHz: allocation.stop_hz,
        allocations: [allocation],
      });
    }
  }
  return groups;
}

function wavelengthLabel(allocations: readonly BandAllocation[]): string | null {
  const labels = allocations
    .flatMap((allocation) => allocation.aliases ?? [])
    .flatMap((alias) => WAVELENGTH_PREFIX.exec(alias.trim())?.[0] ?? [])
    .toSorted((first, second) => first.length - second.length || first.localeCompare(second));
  return labels[0] ?? null;
}

function frequencyLabel(hz: number): string {
  const scale = FREQUENCY_UNITS.find((entry) => hz >= entry.factor);
  if (scale === undefined) {
    return `${Math.round(hz)} Hz`;
  }
  return `${Number((hz / scale.factor).toFixed(3))} ${scale.unit}`;
}
