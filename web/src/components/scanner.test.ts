import { describe, expect, it } from "vitest";
import type {
  BandAllocation,
  BandPlan,
  DeviceSet,
  ScannerStatus,
  ScanSettings,
  TemplateInfo,
} from "../lib/types";
import { rankDevices } from "./devices";
import {
  applyPreset,
  channelName,
  formatDb,
  formatMhz,
  liveStatus,
  presetIn,
  rangeProblem,
  scanPresets,
  stateLabel,
  sweepKind,
  targetCount,
  withHz,
  withoutHz,
} from "./scanner";
import { supports, templatesHint } from "./templates";

const SETTINGS: ScanSettings = {
  channel: 1,
  ranges: [{ start_hz: 145_600_000, stop_hz: 145_800_000, step_hz: 12_500 }],
};

describe("rangeProblem", () => {
  it("accepts a forward range or a channel list", () => {
    expect(rangeProblem(SETTINGS)).toBeNull();
    expect(rangeProblem({ channel: 1, ranges: [], frequencies: [227_360_000] })).toBeNull();
  });

  it("names the reversed line when there is more than one", () => {
    const reversed = { start_hz: 2, stop_hz: 1, step_hz: 1 };
    expect(rangeProblem({ channel: 1, ranges: [reversed] })).toMatch(/^the stop frequency/);
    expect(rangeProblem({ channel: 1, ranges: [...(SETTINGS.ranges ?? []), reversed] })).toMatch(
      /^range 2: /,
    );
    expect(rangeProblem({ channel: 1, ranges: [] })).toMatch(/add a range/);
  });
});

describe("targetCount", () => {
  it("counts inclusively, matching the server's expansion, plus listed channels", () => {
    expect(
      targetCount({ channel: 1, ranges: [{ start_hz: 100, stop_hz: 200, step_hz: 50 }] }),
    ).toBe(3);
    expect(
      targetCount({ channel: 1, ranges: [{ start_hz: 100, stop_hz: 249, step_hz: 50 }] }),
    ).toBe(3);
    expect(
      targetCount({
        channel: 1,
        ranges: [{ start_hz: 100, stop_hz: 200, step_hz: 50 }],
        frequencies: [300, 400],
      }),
    ).toBe(5);
  });
});

function allocation(overrides: Partial<BandAllocation>): BandAllocation {
  return {
    id: "a",
    layer: "world",
    start_hz: 0,
    stop_hz: 1,
    service: "broadcast",
    name: "Band",
    official_name: "BAND",
    aliases: ["band"],
    ...overrides,
  };
}

function plan(allocations: BandAllocation[]): BandPlan {
  return {
    region: { id: "de", name: "Germany", itu_region: "r1", layers: [] },
    layers: [],
    allocations,
    lanes: [],
  };
}

describe("scanPresets", () => {
  const dab = allocation({
    id: "dab",
    name: "VHF Band III: DAB",
    start_hz: 174e6,
    stop_hz: 230e6,
    channels: [
      { name: "5C", hz: 178_352_000 },
      { name: "5A", hz: 174_928_000 },
    ],
  });
  const fm = allocation({
    id: "fm",
    name: "FM broadcast",
    start_hz: 87.5e6,
    stop_hz: 108e6,
    channel_step_hz: 100_000,
  });

  it("offers named bands that carry a raster, lowest first", () => {
    const presets = scanPresets(
      plan([dab, fm, allocation({ id: "x", aliases: [], channel_step_hz: 1 })]),
    );
    expect(presets.map((preset) => preset.name)).toEqual(["FM broadcast", "VHF Band III: DAB"]);
    expect(presets[0]?.ranges).toEqual([{ start_hz: 87.5e6, stop_hz: 108e6, step_hz: 100_000 }]);
    expect(presets[1]?.frequencies).toEqual([174_928_000, 178_352_000]);
    expect(presets[1]?.ranges).toEqual([]);
  });

  it("merges the pieces a band plan splits one band into", () => {
    const low = allocation({ ...fm, id: "fm1", stop_hz: 100e6 });
    const high = allocation({ ...fm, id: "fm2", start_hz: 100e6 });
    expect(scanPresets(plan([low, high]))[0]?.ranges[0]).toMatchObject({
      start_hz: 87.5e6,
      stop_hz: 108e6,
    });
  });

  it("recognises the preset a scan was filled from", () => {
    const presets = scanPresets(plan([dab, fm]));
    const dabPreset = presets[1];
    if (dabPreset === undefined) {
      throw new Error("no DAB preset");
    }
    const filled = applyPreset(SETTINGS, dabPreset);
    expect(presetIn(filled, presets)?.name).toBe("VHF Band III: DAB");
    expect(presetIn(SETTINGS, presets)).toBeNull();
    expect(channelName(plan([dab]), 178_352_000)).toBe("5C");
    expect(channelName(plan([dab]), 178_000_000)).toBeNull();
  });
});

describe("frequency lists", () => {
  it("adds once, in order, and removes", () => {
    expect(withHz([300, 100], 200)).toEqual([100, 200, 300]);
    expect(withHz([100], 100)).toEqual([100]);
    expect(withoutHz([100, 200], 100)).toEqual([200]);
    expect(withoutHz(undefined, 100)).toEqual([]);
  });
});

const STATUS: ScannerStatus = {
  state: "scanning",
  settings: {
    channel: 1,
    ranges: [],
    frequencies: [],
    dwell_ms: 250,
    resume_ms: 1500,
  },
  targets: 10,
  current_hz: 145_500_000,
  sweeps: 0,
  hits: 0,
};

function deviceSet(overrides: Partial<DeviceSet> = {}): DeviceSet {
  return {
    id: 1,
    device: { driver: "virtual", key: "band", label: "Test band" },
    capabilities: {
      freq_ranges: [],
      sample_rates: [],
      gains: [],
      antennas: [],
      bandwidths: [],
      extra: [],
      duplex: "rx_only",
    },
    settings: {},
    status: "running",
    channels: [],
    overruns: 0,
    ...overrides,
  };
}

describe("liveStatus", () => {
  it("prefers the pushed update but falls back to the snapshot", () => {
    const set = deviceSet({ scanners: [STATUS] });
    expect(liveStatus(set, 1, undefined)).toBe(STATUS);
    const pushed = { ...STATUS, current_hz: 146_000_000 };
    expect(liveStatus(set, 1, pushed)).toBe(pushed);
  });

  it("reports nothing when the decoder is not scanning", () => {
    expect(liveStatus(deviceSet({ scanners: [STATUS] }), 2, STATUS)).toBeNull();
    expect(liveStatus(deviceSet(), 1, STATUS)).toBeNull();
    expect(liveStatus(null, 1, STATUS)).toBeNull();
  });
});

describe("stateLabel", () => {
  it("calls an All scan's hold a visit and says when it is done", () => {
    expect(stateLabel(STATUS)).toBe("scanning");
    expect(stateLabel({ ...STATUS, state: "holding" })).toBe("holding");
    const all = { ...STATUS, settings: { ...STATUS.settings, mode: "all" as const } };
    expect(stateLabel({ ...all, state: "holding" })).toBe("visiting");
    expect(stateLabel({ ...all, state: "done" })).toBe("done");
  });
});

describe("formatDb", () => {
  it("renders a dash rather than a bogus number for an absent level", () => {
    expect(formatDb(-31.5)).toBe("-31.5 dB");
    expect(formatDb(null)).toBe("-");
    expect(formatDb(undefined)).toBe("-");
    expect(formatDb(Number.NEGATIVE_INFINITY)).toBe("-");
  });
});

const TEMPLATE: TemplateInfo = {
  id: "adsb",
  name: "Aircraft",
  description: "",
  explainer: "",
  center_hz: 1_090_000_000,
  sample_rate: 2_400_000,
  channels: [],
  min_freq_hz: 1_090_000_000,
  max_freq_hz: 1_090_000_000,
  direction: "rx",
  supported_devices: ["rtlsdr:00000001"],
};

describe("template support", () => {
  it("offers the template on a radio the server listed", () => {
    const rtl = deviceSet({
      device: { driver: "rtlsdr", key: "00000001", label: "RTL-SDR" },
    });
    expect(supports(TEMPLATE, rtl)).toBe(true);
  });

  it("refuses a radio the server left out, and refuses with no radio open", () => {
    const other = deviceSet({
      device: { driver: "rtlsdr", key: "00000002", label: "RTL-SDR" },
    });
    expect(supports(TEMPLATE, other)).toBe(false);
    expect(supports(TEMPLATE, null)).toBe(false);
  });
});

describe("templatesHint", () => {
  it("asks for a device, and says when the open one runs none of them", () => {
    const rtl = deviceSet({ device: { driver: "rtlsdr", key: "00000001", label: "RTL-SDR" } });
    const other = deviceSet({ device: { driver: "rtlsdr", key: "00000002", label: "Other" } });
    expect(templatesHint([TEMPLATE], null)).toBe("Select a device first.");
    expect(templatesHint([TEMPLATE], rtl)).toBeNull();
    expect(templatesHint([TEMPLATE], other)).toBe("Other cannot run these templates.");
  });
});

describe("rankDevices", () => {
  it("puts real hardware above the virtual devices", () => {
    const ranked = rankDevices([
      { driver: "virtual", key: "band", label: "Test band" },
      { driver: "rtlsdr", key: "0001", label: "RTL-SDR" },
      { driver: "virtual", key: "file:a", label: "A recording" },
      { driver: "hackrf", key: "abcd", label: "HackRF One" },
    ]);
    expect(ranked.map((d) => d.label)).toEqual([
      "HackRF One",
      "RTL-SDR",
      "A recording",
      "Test band",
    ]);
  });
});

describe("formatMhz", () => {
  it("shows a dash rather than a frequency nobody reported", () => {
    expect(formatMhz(null)).toBe("-");
    expect(formatMhz(undefined)).toBe("-");
    expect(formatMhz(Number.NaN)).toBe("-");
    expect(formatMhz(145_500_000)).toBe("145.5000 MHz");
  });
});

describe("sweepKind", () => {
  it("says what is actually doing the sweeping, not what was asked for", () => {
    const radio = { capabilities: { hardware_sweep: true } } as DeviceSet;
    expect(sweepKind(radio, null)).toBe("the radio's own");
    expect(sweepKind(radio, { hardware_sweep: false } as never)).toBe("by retuning");
    expect(sweepKind(null, null)).toBe("by retuning");
  });
});
