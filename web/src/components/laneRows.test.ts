import { describe, expect, it } from "vitest";
import type { Capabilities, DeviceSet, GainStage } from "../lib/types";
import {
  allLanesGain,
  inputOptions,
  laneGain,
  laneGains,
  laneInputName,
  laneLayout,
  laneTitle,
  meterStage,
  pickedInputs,
  rxStages,
  spreadOf,
  switching,
  txLaneNames,
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

describe("txLaneNames", () => {
  it("names one TX row on a single lane", () => {
    expect(txLaneNames(capabilities({ gains: [tuner, tx], tx_streams: 1 }))).toEqual(["TX"]);
  });

  it("names a TX row per lane once lanes are set apart", () => {
    const caps = capabilities({
      gains: [tuner, tx],
      rx_streams: 2,
      tx_streams: 2,
      per_stream: { gain: true },
    });
    expect(txLaneNames(caps)).toEqual(["TX1", "TX2"]);
  });

  it("numbers the TX row of a radio with picked inputs, so splitting only adds rows", () => {
    const caps = capabilities({ gains: [tuner, tx], tx_streams: 1, rx_inputs: ["RX1", "RX2"] });
    expect(txLaneNames(caps)).toEqual(["TX1"]);
  });

  it("keeps one TX row while lanes share their gain", () => {
    const caps = capabilities({ gains: [tuner, tx], rx_streams: 2, tx_streams: 2 });
    expect(txLaneNames(caps)).toEqual(["TX"]);
  });
});

describe("laneTitle", () => {
  const inputs = capabilities({ rx_inputs: ["RX1", "RX2"] });

  it("names a lone lane by its input, as it will be named once split", () => {
    expect(laneTitle(deviceSet(inputs, { rx_inputs: [0] }), 0)).toBe("RX1");
    expect(laneTitle(deviceSet(inputs, { rx_inputs: [1] }), 0)).toBe("RX2");
  });

  it("keeps a lane's input name while the radio drops it", () => {
    const split = capabilities({
      rx_inputs: ["RX1", "RX2"],
      rx_streams: 2,
      per_stream: { gain: true },
    });
    expect(laneTitle(deviceSet(split, { rx_inputs: [0] }), 1)).toBe("RX2");
  });

  it("leaves a radio without inputs or lanes unnamed", () => {
    expect(laneTitle(deviceSet(capabilities()), 0)).toBeUndefined();
  });
});

describe("switching", () => {
  const inputs = capabilities({ rx_inputs: ["RX1", "RX2"], rx_streams: 1 });

  it("holds while the radio has not yet opened the lanes asked for", () => {
    expect(switching(deviceSet(inputs, { rx_inputs: [0, 1] }))).toBe(true);
    expect(switching(deviceSet(inputs, { rx_inputs: [0] }))).toBe(false);
  });
});
