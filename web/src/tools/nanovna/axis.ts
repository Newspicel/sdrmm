export interface Tick {
  value: number;
  label: string;
}

const MAX_TICKS = 1000;

const FREQUENCY_UNITS = [
  { factor: 1e9, unit: "GHz" },
  { factor: 1e6, unit: "MHz" },
  { factor: 1e3, unit: "kHz" },
  { factor: 1, unit: "Hz" },
] as const;

export function niceStep(span: number, target: number): number {
  if (!Number.isFinite(span) || !Number.isFinite(target) || span <= 0) {
    return 0;
  }
  const raw = span / Math.max(1, target);
  const decade = 10 ** Math.floor(Math.log10(raw));
  const normalised = raw / decade;
  if (normalised < 1.5) {
    return decade;
  }
  if (normalised < 3.5) {
    return 2 * decade;
  }
  return normalised < 7.5 ? 5 * decade : 10 * decade;
}

export function linearTicks(
  low: number,
  high: number,
  target: number,
  format?: (value: number) => string,
): Tick[] {
  const values = tickValues(low, high, target);
  const step = niceStep(Math.abs(high - low), target);
  const label = format ?? ((value: number) => trimZeros(value.toFixed(decimalsFor(step))));
  return values.map((value) => ({ value, label: label(value) }));
}

export function frequencyTicks(lowHz: number, highHz: number, target: number): Tick[] {
  const values = tickValues(lowHz, highHz, target);
  const step = niceStep(Math.abs(highHz - lowHz), target);
  const scale = frequencyUnit(Math.max(Math.abs(lowHz), Math.abs(highHz)));
  const decimals = decimalsFor(step / scale.factor);
  return values.map((value) => ({
    value,
    label: `${trimZeros((value / scale.factor).toFixed(decimals))} ${scale.unit}`,
  }));
}

function tickValues(low: number, high: number, target: number): number[] {
  if (!Number.isFinite(low) || !Number.isFinite(high) || low === high) {
    return [];
  }
  const lower = Math.min(low, high);
  const upper = Math.max(low, high);
  const step = niceStep(upper - lower, target);
  if (step <= 0) {
    return [];
  }
  const first = Math.ceil(lower / step - 1e-9);
  const last = Math.floor(upper / step + 1e-9);
  if (last - first + 1 > MAX_TICKS) {
    return [];
  }
  const values: number[] = [];
  for (let index = first; index <= last; index += 1) {
    values.push(Number((index * step).toPrecision(12)));
  }
  return values;
}

function frequencyUnit(hz: number): { factor: number; unit: string } {
  return FREQUENCY_UNITS.find((entry) => hz >= entry.factor) ?? { factor: 1, unit: "Hz" };
}

function decimalsFor(step: number): number {
  if (!(step > 0)) {
    return 0;
  }
  return Math.max(0, Math.ceil(-Math.log10(step) - 1e-6));
}

function trimZeros(text: string): string {
  const trimmed = text.includes(".") ? text.replace(/\.?0+$/, "") : text;
  return trimmed === "-0" ? "0" : trimmed;
}
