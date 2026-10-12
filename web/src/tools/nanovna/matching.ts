import type { NanoVnaComplex } from "../../lib/types";
import { REFERENCE_OHMS } from "./nanovna";

export interface MatchElement {
  kind: "inductor" | "capacitor";
  value: number;
  reactanceOhms: number;
}

export interface LNetwork {
  shuntAt: "load" | "source";
  series: MatchElement;
  shunt: MatchElement;
}

const MATCHED_TOLERANCE = 0.005;
const NEGLIGIBLE = 1e-9;

export function lNetworks(
  load: NanoVnaComplex,
  frequencyHz: number,
  referenceOhms = REFERENCE_OHMS,
): LNetwork[] {
  if (!solvable(load, frequencyHz, referenceOhms)) {
    return [];
  }
  const omega = 2 * Math.PI * frequencyHz;
  return [...shuntAtLoad(load, referenceOhms, omega), ...shuntAtSource(load, referenceOhms, omega)];
}

export function inputImpedance(
  network: LNetwork,
  load: NanoVnaComplex,
  frequencyHz: number,
): NanoVnaComplex {
  const omega = 2 * Math.PI * frequencyHz;
  const series = elementImpedance(network.series, omega);
  const shunt = elementImpedance(network.shunt, omega);
  return network.shuntAt === "load"
    ? add(parallel(load, shunt), series)
    : parallel(add(load, series), shunt);
}

function solvable(load: NanoVnaComplex, frequencyHz: number, referenceOhms: number): boolean {
  if (![load.re, load.im, frequencyHz, referenceOhms].every(Number.isFinite)) {
    return false;
  }
  if (load.re <= 0 || frequencyHz <= 0 || referenceOhms <= 0) {
    return false;
  }
  return Math.hypot(load.re - referenceOhms, load.im) > MATCHED_TOLERANCE * referenceOhms;
}

function shuntAtLoad(load: NanoVnaComplex, referenceOhms: number, omega: number): LNetwork[] {
  const admittance = inverse(load);
  const conductance = admittance.re;
  const squared = conductance / referenceOhms - conductance * conductance;
  return roots(squared).flatMap((susceptance) => {
    const shuntSusceptance = susceptance - admittance.im;
    const seriesReactance = susceptance / (conductance * conductance + susceptance * susceptance);
    return networkOf("load", seriesReactance, shuntSusceptance, omega);
  });
}

function shuntAtSource(load: NanoVnaComplex, referenceOhms: number, omega: number): LNetwork[] {
  const squared = load.re * referenceOhms - load.re * load.re;
  return roots(squared).flatMap((reactance) => {
    const seriesReactance = reactance - load.im;
    const shuntSusceptance = reactance / (load.re * load.re + reactance * reactance);
    return networkOf("source", seriesReactance, shuntSusceptance, omega);
  });
}

function roots(squared: number): number[] {
  if (squared <= 0) {
    return [];
  }
  const root = Math.sqrt(squared);
  return [root, -root];
}

function networkOf(
  shuntAt: LNetwork["shuntAt"],
  seriesReactance: number,
  shuntSusceptance: number,
  omega: number,
): LNetwork[] {
  const series = seriesElement(seriesReactance, omega);
  const shunt = shuntElement(shuntSusceptance, omega);
  return series === null || shunt === null ? [] : [{ shuntAt, series, shunt }];
}

function seriesElement(reactanceOhms: number, omega: number): MatchElement | null {
  if (Math.abs(reactanceOhms) < NEGLIGIBLE) {
    return null;
  }
  return reactanceOhms > 0
    ? { kind: "inductor", value: reactanceOhms / omega, reactanceOhms }
    : { kind: "capacitor", value: -1 / (omega * reactanceOhms), reactanceOhms };
}

function shuntElement(susceptance: number, omega: number): MatchElement | null {
  if (Math.abs(susceptance) < NEGLIGIBLE) {
    return null;
  }
  return susceptance > 0
    ? { kind: "capacitor", value: susceptance / omega, reactanceOhms: -1 / susceptance }
    : { kind: "inductor", value: -1 / (omega * susceptance), reactanceOhms: -1 / susceptance };
}

function elementImpedance(element: MatchElement, omega: number): NanoVnaComplex {
  return element.kind === "inductor"
    ? { re: 0, im: omega * element.value }
    : { re: 0, im: -1 / (omega * element.value) };
}

function add(first: NanoVnaComplex, second: NanoVnaComplex): NanoVnaComplex {
  return { re: first.re + second.re, im: first.im + second.im };
}

function inverse(value: NanoVnaComplex): NanoVnaComplex {
  const squared = value.re * value.re + value.im * value.im;
  return { re: value.re / squared, im: -value.im / squared };
}

function parallel(first: NanoVnaComplex, second: NanoVnaComplex): NanoVnaComplex {
  return inverse(add(inverse(first), inverse(second)));
}
