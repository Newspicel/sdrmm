import { describe, expect, it } from "vitest";
import type { Capabilities, DeviceSet, GainStage } from "../lib/types";
import {
  allLanesGain,
  inputOptions,
  laneGain,
  laneGains,
  laneInputName,
  laneLayout,
  meterStage,
  pickedInputs,
  rxStages,
  spreadOf,
  txStages,
} from "./laneRows";

const tuner: GainStage = { name: "tuner", kind: "tuner", range: { min: 0, max: 49.6, step: 0.1 } };
const amp: GainStage = { name: "amp", kind: "amp", range: { min: 0, max: 14 } };
const tx: GainStage = {
  name: "tx",
  kind: "tx",
  range: { min: -89, max: 0 },
  agc: { kind: "never" },
};

function capabilities(overrides: Partial<Capabilities> = {}): Capabilities {
  return {
    freq_ranges: [],
    sample_rates: [],
    gains: [tuner],
    antennas: [],
    bandwidths: [],
    extra: [],
    duplex: "rx_only",
    ...overrides,
  };
}

function deviceSet(caps: Capabilities, settings: DeviceSet["settings"] = {}): DeviceSet {
  return {
    id: 1,
    device: { driver: "virtual", key: "band", label: "Test" },
    capabilities: caps,
    settings,
    status: "running",
    channels: [],
    overruns: 0,
  };
}

describe("laneLayout", () => {
  it("draws one row for a single tuner", () => {
    expect(laneLayout(capabilities())).toEqual({
      lanes: 1,
      perLane: false,
      master: false,
      inputs: false,
    });
  });

  it("draws a master row above per-lane gains", () => {
    expect(laneLayout(capabilities({ rx_streams: 5, per_stream: { gain: true } }))).toEqual({
      lanes: 5,
      perLane: true,
      master: true,
      inputs: false,
    });
  });

  it("keeps a shared gain on one row even with several streams", () => {
    expect(laneLayout(capabilities({ rx_streams: 4 })).lanes).toBe(1);
  });

  it("offers input tabs when the radio lets you pick receivers", () => {
    const layout = laneLayout(
      capabilities({ rx_streams: 1, rx_inputs: ["RX1", "RX2"], per_stream: { gain: true } }),
    );
    expect(layout).toEqual({ lanes: 1, perLane: false, master: false, inputs: true });
  });
});

describe("stages", () => {
  it("keeps transmit gain off the receive lanes", () => {
    const caps = capabilities({ gains: [amp, tuner, tx] });
    expect(rxStages(caps)).toEqual([amp, tuner]);
    expect(txStages(caps)).toEqual([tx]);
  });

  it("puts the meter on the stage the AGC drives, never on a switch", () => {
    expect(meterStage(capabilities({ gains: [amp, tuner] }))).toBe(tuner);
    expect(meterStage(capabilities({ gains: [amp] }))).toBeUndefined();
  });
});

describe("lane gains", () => {
  const caps = capabilities({ rx_streams: 2, per_stream: { gain: true } });

  it("reads each lane's own value and falls back to the stage floor", () => {
    const set = deviceSet(caps, {
      gains: [{ stage: "tuner", value_db: 20 }],
      streams: [{ stream: 1, gains: [{ stage: "tuner", value_db: 30 }] }],
    });
    expect(laneGains(set, tuner)).toEqual([20, 30]);
    expect(laneGains(deviceSet(caps), tuner)).toEqual([0, 0]);
  });

  it("writes one lane or every lane", () => {
    expect(laneGain(caps, 1, tuner, 12)).toEqual({
      streams: [{ stream: 1, gains: [{ stage: "tuner", value_db: 12 }] }],
    });
    expect(allLanesGain(caps, tuner, 12)).toEqual({
      streams: [0, 1].map((stream) => ({ stream, gains: [{ stage: "tuner", value_db: 12 }] })),
    });
    expect(allLanesGain(capabilities(), tuner, 12)).toEqual({
      gains: [{ stage: "tuner", value_db: 12 }],
    });
  });

  it("tells a uniform spread from a mixed one", () => {
    expect(spreadOf([20, 20])).toEqual({ uniform: true, mean: 20 });
    expect(spreadOf([10, 30])).toEqual({ uniform: false, mean: 20 });
    expect(spreadOf([])).toEqual({ uniform: true, mean: 0 });
  });
});

describe("rx inputs", () => {
  const caps = capabilities({ rx_streams: 1, rx_inputs: ["RX1", "RX2"] });

  it("offers each receiver as its own toggle", () => {
    expect(inputOptions(caps)).toEqual([
      { value: 0, label: "RX1" },
      { value: 1, label: "RX2" },
    ]);
    expect(inputOptions(capabilities())).toEqual([]);
  });

  it("reads the pick back with the first receiver as the default", () => {
    expect(pickedInputs({})).toEqual([0]);
    expect(pickedInputs({ rx_inputs: [0, 1] })).toEqual([0, 1]);
  });

  it("names each lane after the receiver it carries", () => {
    const set = deviceSet(caps, { rx_inputs: [1] });
    expect(laneInputName(set, 0)).toBe("RX2");
    expect(laneInputName(deviceSet(capabilities()), 0)).toBeUndefined();
  });
});
