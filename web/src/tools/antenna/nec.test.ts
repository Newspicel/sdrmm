import { describe, expect, it } from "vitest";
import type { AntennaReport, AntennaSimulation } from "../../lib/types";
import type { ModelSettings } from "./antenna";
import { necDeck, necFilename } from "./nec";

const REPORT: AntennaReport = {
  design: { type: "dipole" },
  frequency_hz: 14_200_000,
  wavelength_m: 21.1,
  velocity_factor: 0.95,
  parts: [],
  geometry: { segments: [], feed: { x_m: 0, y_m: 0, z_m: 0 } },
  balanced: true,
  notes: [],
  simulation: { status: "unavailable", reason: "" },
};

const SIMULATION: AntennaSimulation = {
  impedance: { re: 70, im: 0 },
  gain_dbi: 2.1,
  efficiency: 0.99,
  takeoff_deg: 0,
  bearing_deg: 0,
  front_to_back_db: 0,
  polarization: "horizontal",
  azimuth: { step_deg: 2, gain_dbi: [] },
  elevation: { step_deg: 2, gain_dbi: [] },
  sweep: [
    { frequency_hz: 14e6, impedance: { re: 60, im: -20 } },
    { frequency_hz: 14.5e6, impedance: { re: 75, im: 20 } },
  ],
  wires: [
    { from: { x_m: -5, y_m: 0, z_m: 0 }, to: { x_m: 0, y_m: 0, z_m: 0 }, segments: 5, current: [] },
    { from: { x_m: 0, y_m: 0, z_m: 0 }, to: { x_m: 5, y_m: 0, z_m: 0 }, segments: 5, current: [] },
  ],
  feed_wire: 1,
  feed_segment: 0,
  wire_radius_m: 0.001,
};

const OVER_GROUND: ModelSettings = {
  wire_diameter_mm: 2,
  material: "copper",
  ground: "average",
  height_m: 10,
};

describe("necDeck", () => {
  const lines = necDeck(REPORT, SIMULATION, OVER_GROUND).split("\n");

  it("writes every wire with z up and the feedpoint at its height", () => {
    expect(lines).toContain("GW 1 5 0 -5 10 0 0 10 0.001");
    expect(lines).toContain("GW 2 5 0 0 10 0 5 10 0.001");
  });

  it("feeds the hot wire at the segment beside the feedpoint", () => {
    expect(lines).toContain("EX 0 2 1 0 1 0");
  });

  it("carries the ground, the metal and the sweep", () => {
    expect(lines).toContain("GE 1");
    expect(lines).toContain("GN 2 0 0 0 13 0.005");
    expect(lines).toContain("LD 5 0 0 0 5.80e+7");
    expect(lines).toContain("FR 0 2 0 0 14 0.5");
    expect(lines.at(-2)).toBe("EN");
  });

  it("models free space without a ground card", () => {
    const free = necDeck(REPORT, SIMULATION, { ...OVER_GROUND, ground: "free_space" });
    expect(free).toContain("GE 0");
    expect(free).toContain("GN -1");
    expect(free).toContain("GW 1 5 0 -5 0 0 0 0 0.001");
  });

  it("names the file after the design and frequency", () => {
    expect(necFilename(REPORT)).toBe("dipole-14200khz.nec");
  });
});
