import { describe, expect, it } from "vitest";
import type { NanoVnaComplex } from "../../lib/types";
import { inputImpedance, type LNetwork, lNetworks, type MatchElement } from "./matching";

const FREQUENCY_HZ = 14_200_000;
const OMEGA = 2 * Math.PI * FREQUENCY_HZ;

const LOADS: ReadonlyArray<{ load: NanoVnaComplex; count: number }> = [
  { load: { re: 12.5, im: -30 }, count: 4 },
  { load: { re: 200, im: 75 }, count: 2 },
  { load: { re: 50, im: 40 }, count: 1 },
  { load: { re: 25, im: 0 }, count: 2 },
  { load: { re: 5, im: 150 }, count: 4 },
];

function expectMatched(network: LNetwork, load: NanoVnaComplex, referenceOhms = 50): void {
  const z = inputImpedance(network, load, FREQUENCY_HZ);
  expect(Math.hypot(z.re - referenceOhms, z.im)).toBeLessThan(referenceOhms * 0.001);
}

function expectConsistent(element: MatchElement): void {
  expect(element.value).toBeGreaterThan(0);
  const reactance =
    element.kind === "inductor" ? OMEGA * element.value : -1 / (OMEGA * element.value);
  expect(element.reactanceOhms).toBeCloseTo(reactance, 6);
}

describe("lNetworks", () => {
  it.each(LOADS)("matches $load.re $load.im ohm to 50 ohm", ({ load, count }) => {
    const networks = lNetworks(load, FREQUENCY_HZ);
    expect(networks).toHaveLength(count);
    for (const network of networks) {
      expectMatched(network, load);
      expectConsistent(network.series);
      expectConsistent(network.shunt);
    }
  });

  it("puts the shunt on the side with the higher resistance", () => {
    expect(lNetworks({ re: 200, im: 75 }, FREQUENCY_HZ).map((network) => network.shuntAt)).toEqual([
      "load",
      "load",
    ]);
    expect(lNetworks({ re: 25, im: 0 }, FREQUENCY_HZ).map((network) => network.shuntAt)).toEqual([
      "source",
      "source",
    ]);
  });

  it("offers a low-pass and a high-pass solution for a resistive load", () => {
    const kinds = lNetworks({ re: 25, im: 0 }, FREQUENCY_HZ).map(
      (network) => `${network.series.kind}/${network.shunt.kind}`,
    );
    expect(kinds.toSorted()).toEqual(["capacitor/inductor", "inductor/capacitor"]);
  });

  it("matches to another reference impedance", () => {
    const load = { re: 300, im: -120 };
    const networks = lNetworks(load, FREQUENCY_HZ, 75);
    expect(networks.length).toBeGreaterThan(0);
    for (const network of networks) {
      expectMatched(network, load, 75);
    }
  });

  it("needs no network for a load that is already matched", () => {
    expect(lNetworks({ re: 50.1, im: 0.1 }, FREQUENCY_HZ)).toEqual([]);
  });

  it("gives up on loads it cannot match", () => {
    expect(lNetworks({ re: 0, im: 20 }, FREQUENCY_HZ)).toEqual([]);
    expect(lNetworks({ re: -10, im: 0 }, FREQUENCY_HZ)).toEqual([]);
    expect(lNetworks({ re: Number.NaN, im: 0 }, FREQUENCY_HZ)).toEqual([]);
    expect(lNetworks({ re: 25, im: 0 }, 0)).toEqual([]);
  });
});
