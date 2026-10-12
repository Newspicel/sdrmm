import type { NanoVnaComplex, NanoVnaPoint } from "../../lib/types";
import { impedance, REFERENCE_OHMS } from "./nanovna";

export type Parsed<T> = { ok: true; value: T } | { ok: false; error: string };

export interface TouchstoneFile {
  ports: 1 | 2;
  referenceOhms: number;
  points: NanoVnaPoint[];
  comments: string[];
}

type DataFormat = "ri" | "ma" | "db";

interface OptionLine {
  multiplier: number;
  format: DataFormat;
  referenceOhms: number;
}

interface Scanned {
  options: OptionLine;
  numbers: number[];
  comments: string[];
}

const UNIT_MULTIPLIERS: Readonly<Record<string, number>> = {
  hz: 1,
  khz: 1e3,
  mhz: 1e6,
  ghz: 1e9,
};

const DATA_FORMATS: readonly string[] = ["ri", "ma", "db"];

const OTHER_PARAMETERS: readonly string[] = ["y", "z", "h", "g"];

const DEFAULT_OPTIONS: OptionLine = { multiplier: 1e9, format: "ma", referenceOhms: 50 };

const ZERO: NanoVnaComplex = { re: 0, im: 0 };

export function touchstonePorts(filename: string): 1 | 2 | null {
  const ports = /\.s([12])p$/i.exec(filename.trim())?.[1];
  if (ports === "1") {
    return 1;
  }
  return ports === "2" ? 2 : null;
}

export function parseTouchstone(text: string, ports: 1 | 2): Parsed<TouchstoneFile> {
  const scanned = scan(text);
  if (!scanned.ok) {
    return scanned;
  }
  const { options, numbers, comments } = scanned.value;
  const points = toPoints(numbers, ports, options);
  if (!points.ok) {
    return points;
  }
  return {
    ok: true,
    value: { ports, referenceOhms: options.referenceOhms, points: points.value, comments },
  };
}

function scan(text: string): Parsed<Scanned> {
  let options: OptionLine | null = null;
  const numbers: number[] = [];
  const comments: string[] = [];
  const lines = text.split(/\r?\n/);
  for (const [index, line] of lines.entries()) {
    const bang = line.indexOf("!");
    const comment = bang < 0 ? "" : line.slice(bang + 1).trim();
    if (comment !== "") {
      comments.push(comment);
    }
    const data = (bang < 0 ? line : line.slice(0, bang)).trim();
    if (data.startsWith("#")) {
      const parsed: Parsed<OptionLine> =
        options === null ? parseOptions(data) : { ok: true, value: options };
      if (!parsed.ok) {
        return parsed;
      }
      options = parsed.value;
    } else if (data.startsWith("[")) {
      return failure("Touchstone 2 files are not supported");
    } else if (data !== "") {
      const added = appendNumbers(data, index + 1, numbers);
      if (!added.ok) {
        return added;
      }
    }
  }
  return { ok: true, value: { options: options ?? DEFAULT_OPTIONS, numbers, comments } };
}

function parseOptions(line: string): Parsed<OptionLine> {
  const tokens = line.slice(1).trim().toLowerCase().split(/\s+/).filter(Boolean);
  const options = { ...DEFAULT_OPTIONS };
  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index] ?? "";
    const multiplier = UNIT_MULTIPLIERS[token];
    if (multiplier !== undefined) {
      options.multiplier = multiplier;
    } else if (isDataFormat(token)) {
      options.format = token;
    } else if (token === "r") {
      index += 1;
      const ohms = Number(tokens[index]);
      if (!(ohms > 0) || !Number.isFinite(ohms)) {
        return failure("Bad reference impedance");
      }
      options.referenceOhms = ohms;
    } else if (OTHER_PARAMETERS.includes(token)) {
      return failure("Only S parameters are supported");
    } else if (token !== "s") {
      return failure(`Unknown option "${token}"`);
    }
  }
  return { ok: true, value: options };
}

function appendNumbers(data: string, lineNumber: number, numbers: number[]): Parsed<null> {
  for (const token of data.split(/\s+/)) {
    const value = Number(token);
    if (!Number.isFinite(value)) {
      return failure(`Line ${lineNumber}: "${token}" is not a number`);
    }
    numbers.push(value);
  }
  return { ok: true, value: null };
}

function toPoints(
  numbers: readonly number[],
  ports: 1 | 2,
  options: OptionLine,
): Parsed<NanoVnaPoint[]> {
  const width = ports === 1 ? 3 : 9;
  if (numbers.length % width !== 0) {
    return failure("Incomplete data row");
  }
  const count = numbers.length / width;
  if (count < 2) {
    return failure("Need at least 2 points");
  }
  const points: NanoVnaPoint[] = [];
  for (let row = 0; row < count; row += 1) {
    const point = toPoint(numbers.slice(row * width, (row + 1) * width), ports, options);
    if (!isFinitePoint(point)) {
      return failure(`Point ${row + 1}: value out of range`);
    }
    const previous = points.at(-1);
    if (previous !== undefined && point.frequency_hz <= previous.frequency_hz) {
      return failure(`Point ${row + 1}: frequency not increasing`);
    }
    points.push(point);
  }
  return { ok: true, value: points };
}

function toPoint(row: readonly number[], ports: 1 | 2, options: OptionLine): NanoVnaPoint {
  const [frequency = 0, s11a = 0, s11b = 0, s21a = 0, s21b = 0] = row;
  const s11 = toComplex(s11a, s11b, options.format);
  return {
    frequency_hz: Math.round(frequency * options.multiplier),
    s11: renormalise(s11, options.referenceOhms),
    s21: ports === 2 ? toComplex(s21a, s21b, options.format) : ZERO,
  };
}

function toComplex(first: number, second: number, format: DataFormat): NanoVnaComplex {
  if (format === "ri") {
    return { re: first, im: second };
  }
  const magnitudeOf = format === "ma" ? first : 10 ** (first / 20);
  const radians = (second * Math.PI) / 180;
  return { re: magnitudeOf * Math.cos(radians), im: magnitudeOf * Math.sin(radians) };
}

function renormalise(value: NanoVnaComplex, fromOhms: number): NanoVnaComplex {
  if (fromOhms === REFERENCE_OHMS) {
    return value;
  }
  const z = impedance(value, fromOhms);
  if (z === null) {
    return value;
  }
  const numerator = { re: z.re - REFERENCE_OHMS, im: z.im };
  const denominator = { re: z.re + REFERENCE_OHMS, im: z.im };
  const squared = denominator.re * denominator.re + denominator.im * denominator.im;
  return {
    re: (numerator.re * denominator.re + numerator.im * denominator.im) / squared,
    im: (numerator.im * denominator.re - numerator.re * denominator.im) / squared,
  };
}

function isFinitePoint(point: NanoVnaPoint): boolean {
  return [point.frequency_hz, point.s11.re, point.s11.im, point.s21.re, point.s21.im].every(
    Number.isFinite,
  );
}

function isDataFormat(token: string): token is DataFormat {
  return DATA_FORMATS.includes(token);
}

function failure(error: string): { ok: false; error: string } {
  return { ok: false, error };
}
