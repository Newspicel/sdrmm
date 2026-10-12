import type { NanoVnaComplex, NanoVnaPoint } from "../../lib/types";
import { REFERENCE_OHMS } from "./nanovna";

export type TdrWindow = "rectangular" | "hann" | "kaiser";

export interface TdrPeak {
  index: number;
  distanceM: number;
  value: number;
}

export interface TdrResult {
  lowpass: boolean;
  distanceM: number[];
  impulse: number[];
  step: number[] | null;
  resolutionM: number;
  rangeM: number;
  peak: TdrPeak | null;
}

export interface TdrOptions {
  window?: TdrWindow;
  padTo?: number;
}

const SPEED_OF_LIGHT = 299_792_458;
const MAX_FFT_SIZE = 65_536;
const DEFAULT_PADDING = 4;
const HARMONIC_TOLERANCE = 0.01;
const KAISER_BETA = 6;
const STEP_LIMIT = 0.999;
const DISTANT_SHARE = 0.5;

const EMPTY_RESULT: TdrResult = {
  lowpass: false,
  distanceM: [],
  impulse: [],
  step: null,
  resolutionM: 0,
  rangeM: 0,
  peak: null,
};

export function fft(re: Float64Array, im: Float64Array, inverse: boolean): void {
  const size = re.length;
  if (size !== im.length || !isPowerOfTwo(size)) {
    throw new RangeError(`FFT size ${size} is not a power of two`);
  }
  bitReverse(re, im);
  butterflies(re, im, inverse);
  if (inverse) {
    for (let index = 0; index < size; index += 1) {
      re[index] = at(re, index) / size;
      im[index] = at(im, index) / size;
    }
  }
}

export function isHarmonicGrid(points: readonly NanoVnaPoint[]): boolean {
  const step = gridStep(points);
  const first = points[0]?.frequency_hz;
  return step > 0 && first !== undefined && Math.abs(first - step) <= HARMONIC_TOLERANCE * step;
}

export function timeDomain(
  points: readonly NanoVnaPoint[],
  velocityFactor: number,
  options: TdrOptions = {},
): TdrResult {
  const step = gridStep(points);
  if (!(step > 0) || !(velocityFactor > 0) || !Number.isFinite(velocityFactor)) {
    return EMPTY_RESULT;
  }
  const window = options.window ?? "kaiser";
  const size = fftSize(2 * points.length + 2, options.padTo ?? DEFAULT_PADDING * points.length);
  const metresPerSecond = (SPEED_OF_LIGHT * velocityFactor) / 2;
  return isHarmonicGrid(points)
    ? lowpass(points, step, size, window, metresPerSecond)
    : bandpass(points, step, size, window, metresPerSecond);
}

function lowpass(
  points: readonly NanoVnaPoint[],
  stepHz: number,
  size: number,
  window: TdrWindow,
  metresPerSecond: number,
): TdrResult {
  const count = Math.min(points.length, size / 2 - 1);
  const weights = Array.from({ length: count + 1 }, (_, index) =>
    windowValue(window, index / (count + 1)),
  );
  const re = new Float64Array(size);
  const im = new Float64Array(size);
  re[0] = dcEstimate(points);
  for (let index = 1; index <= count; index += 1) {
    const sample = scaled(points[index - 1]?.s11, weights[index] ?? 0);
    re[index] = sample.re;
    im[index] = sample.im;
    re[size - index] = sample.re;
    im[size - index] = -sample.im;
  }
  fft(re, im, true);
  const weightSum = weights.reduce((sum, weight, index) => sum + (index === 0 ? 1 : 2) * weight, 0);
  const impulse = Array.from(re, (value) => (value * size) / weightSum);
  const distances = distanceAxis(size, stepHz, metresPerSecond);
  const resolutionM = metresPerSecond / (2 * count * stepHz);
  const rangeM = metresPerSecond / stepHz;
  return {
    lowpass: true,
    distanceM: distances,
    impulse,
    step: stepImpedance(re),
    resolutionM,
    rangeM,
    peak: findPeak(impulse, distances, resolutionM, rangeM),
  };
}

function bandpass(
  points: readonly NanoVnaPoint[],
  stepHz: number,
  size: number,
  window: TdrWindow,
  metresPerSecond: number,
): TdrResult {
  const count = Math.min(points.length, size);
  const re = new Float64Array(size);
  const im = new Float64Array(size);
  let weightSum = 0;
  for (let index = 0; index < count; index += 1) {
    const weight = windowValue(window, (2 * index + 1 - count) / count);
    const sample = scaled(points[index]?.s11, weight);
    re[index] = sample.re;
    im[index] = sample.im;
    weightSum += weight;
  }
  fft(re, im, true);
  const impulse = Array.from(
    re,
    (value, index) => (Math.hypot(value, at(im, index)) * size) / weightSum,
  );
  const distances = distanceAxis(size, stepHz, metresPerSecond);
  const resolutionM = metresPerSecond / ((count - 1) * stepHz);
  const rangeM = metresPerSecond / stepHz;
  return {
    lowpass: false,
    distanceM: distances,
    impulse,
    step: null,
    resolutionM,
    rangeM,
    peak: findPeak(impulse, distances, resolutionM, rangeM),
  };
}

function findPeak(
  impulse: readonly number[],
  distances: readonly number[],
  resolutionM: number,
  rangeM: number,
): TdrPeak | null {
  const unambiguous = (index: number) => (distances[index] ?? 0) <= rangeM / 2;
  const overall = strongest(impulse, unambiguous);
  if (overall === null) {
    return null;
  }
  const away = strongest(
    impulse,
    (index) => unambiguous(index) && (distances[index] ?? 0) >= 2 * resolutionM,
  );
  const floor = Math.abs(impulse[overall] ?? 0) * DISTANT_SHARE;
  const index = away !== null && Math.abs(impulse[away] ?? 0) >= floor ? away : overall;
  return { index, distanceM: distances[index] ?? 0, value: impulse[index] ?? 0 };
}

function strongest(values: readonly number[], include: (index: number) => boolean): number | null {
  let best: number | null = null;
  let bestMagnitude = 0;
  values.forEach((value, index) => {
    const magnitudeOf = Math.abs(value);
    if (include(index) && magnitudeOf > bestMagnitude) {
      best = index;
      bestMagnitude = magnitudeOf;
    }
  });
  return best;
}

function stepImpedance(impulse: Float64Array): number[] {
  let sum = 0;
  return Array.from(impulse, (value) => {
    sum += value;
    const reflection = Math.min(STEP_LIMIT, Math.max(-STEP_LIMIT, sum));
    return (REFERENCE_OHMS * (1 + reflection)) / (1 - reflection);
  });
}

function dcEstimate(points: readonly NanoVnaPoint[]): number {
  const first = points[0]?.s11.re ?? 0;
  const second = points[1]?.s11.re ?? first;
  return Math.min(1, Math.max(-1, 2 * first - second));
}

function distanceAxis(size: number, stepHz: number, metresPerSecond: number): number[] {
  const sampleSeconds = 1 / (size * stepHz);
  return Array.from({ length: size }, (_, index) => index * sampleSeconds * metresPerSecond);
}

function gridStep(points: readonly NanoVnaPoint[]): number {
  const first = points[0]?.frequency_hz;
  const last = points[points.length - 1]?.frequency_hz;
  if (first === undefined || last === undefined || points.length < 2) {
    return 0;
  }
  return (last - first) / (points.length - 1);
}

function fftSize(minimum: number, requested: number): number {
  const wanted = Math.max(minimum, Number.isFinite(requested) ? requested : 0);
  let size = 1;
  while (size < wanted && size < MAX_FFT_SIZE) {
    size *= 2;
  }
  return size;
}

function windowValue(kind: TdrWindow, position: number): number {
  if (kind === "hann") {
    return 0.5 * (1 + Math.cos(Math.PI * position));
  }
  if (kind === "kaiser") {
    return (
      besselI0(KAISER_BETA * Math.sqrt(Math.max(0, 1 - position * position))) /
      besselI0(KAISER_BETA)
    );
  }
  return 1;
}

function besselI0(x: number): number {
  let sum = 1;
  let term = 1;
  for (let k = 1; term > 1e-12 * sum; k += 1) {
    term *= (x / (2 * k)) ** 2;
    sum += term;
  }
  return sum;
}

function scaled(value: NanoVnaComplex | undefined, weight: number): NanoVnaComplex {
  return value === undefined ? { re: 0, im: 0 } : { re: value.re * weight, im: value.im * weight };
}

function isPowerOfTwo(size: number): boolean {
  return Number.isInteger(size) && size > 0 && (size & (size - 1)) === 0;
}

function bitReverse(re: Float64Array, im: Float64Array): void {
  const size = re.length;
  for (let index = 1, reversed = 0; index < size; index += 1) {
    let bit = size >> 1;
    while (reversed & bit) {
      reversed ^= bit;
      bit >>= 1;
    }
    reversed ^= bit;
    if (index < reversed) {
      swap(re, index, reversed);
      swap(im, index, reversed);
    }
  }
}

function butterflies(re: Float64Array, im: Float64Array, inverse: boolean): void {
  const size = re.length;
  const sign = inverse ? 1 : -1;
  for (let length = 2; length <= size; length *= 2) {
    const half = length / 2;
    const angle = (sign * 2 * Math.PI) / length;
    for (let start = 0; start < size; start += length) {
      for (let offset = 0; offset < half; offset += 1) {
        const twiddleRe = Math.cos(angle * offset);
        const twiddleIm = Math.sin(angle * offset);
        const top = start + offset;
        const bottom = top + half;
        const productRe = at(re, bottom) * twiddleRe - at(im, bottom) * twiddleIm;
        const productIm = at(re, bottom) * twiddleIm + at(im, bottom) * twiddleRe;
        re[bottom] = at(re, top) - productRe;
        im[bottom] = at(im, top) - productIm;
        re[top] = at(re, top) + productRe;
        im[top] = at(im, top) + productIm;
      }
    }
  }
}

function swap(values: Float64Array, first: number, second: number): void {
  const held = at(values, first);
  values[first] = at(values, second);
  values[second] = held;
}

function at(values: Float64Array, index: number): number {
  return values[index] ?? 0;
}
