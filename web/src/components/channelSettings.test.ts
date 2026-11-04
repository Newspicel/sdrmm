import { describe, expect, it } from "vitest";
import wire from "../generated/limits.json" with { type: "json" };
import type { AudioProcessing, ChannelDescriptor, ChannelSettings } from "../lib/types";
import {
  AUDIO_DEFAULTS,
  AUDIO_LIMITS,
  audioChainActive,
  channelHasAudio,
  channelWidthHz,
  clampOffsetHz,
  IF_LIMITS,
  ifEdgeHz,
  ifFilterSummary,
  mergeAudio,
  mergeChannelSettings,
  nudgedSquelch,
  offsetForFrequencyHz,
  offsetLimitHz,
  paramsWith,
  radioWindowHz,
  reachesHz,
  squelchAt,
  squelchLevelDb,
  squelchMarginDb,
  squelchMode,
  withIfEdge,
  withIfNotchAdded,
  withIfNotchRemoved,
  withNotchAdded,
  withNotchAt,
  withNotchRemoved,
} from "./channelSettings";

const base: ChannelSettings = {
  frequency_hz: 145_025_000,
  squelch: { mode: "manual", level_db: -70 },
  params: paramsWith("probe", { sideband: "lsb", bandwidth_hz: 2_400 }),
};

function descriptor(over: Partial<ChannelDescriptor>): ChannelDescriptor {
  return {
    type_id: "probe",
    name: "Probe",
    bandwidth_hz: 12_500,
    input_rate_hz: 48_000,
    ...over,
  };
}

describe("mergeChannelSettings", () => {
  it("widens a frequency edit and keeps squelch + params", () => {
    expect(mergeChannelSettings(base, { frequency_hz: 144_987_500 })).toEqual({
      ...base,
      blanker: {},
      if_filter: {},
      frequency_hz: 144_987_500,
    });
  });

  it("carries the blanker through an unrelated edit", () => {
    const blanked: ChannelSettings = { ...base, blanker: { enabled: true, threshold: 7 } };
    expect(mergeChannelSettings(blanked, { frequency_hz: 145_000_000 }).blanker).toEqual({
      enabled: true,
      threshold: 7,
    });
  });

  it("carries the IF filter through an unrelated edit", () => {
    const shaped: ChannelSettings = { ...base, if_filter: { enabled: true, high_hz: 40_000 } };
    expect(mergeChannelSettings(shaped, { frequency_hz: 145_000_000 }).if_filter).toEqual({
      enabled: true,
      high_hz: 40_000,
    });
  });

  it("replaces the squelch whole and keeps it when the edit says nothing", () => {
    expect(mergeChannelSettings(base, { squelch: { mode: "off" } }).squelch).toEqual({
      mode: "off",
    });
    expect(mergeChannelSettings(base, {}).squelch).toEqual({ mode: "manual", level_db: -70 });
    expect(mergeChannelSettings(base, { squelch: { mode: "auto", margin_db: 8 } }).squelch).toEqual(
      { mode: "auto", margin_db: 8 },
    );
  });

  it("swaps params wholesale without touching placement", () => {
    const next = mergeChannelSettings(base, {
      params: paramsWith("other", { bandwidth_hz: 25_000 }),
    });
    expect(next.frequency_hz).toBe(145_025_000);
    expect(next.squelch).toEqual({ mode: "manual", level_db: -70 });
    expect(next.params).toEqual(paramsWith("other", { bandwidth_hz: 25_000 }));
  });

  it("fills server defaults for absent optional fields", () => {
    const sparse: ChannelSettings = {
      frequency_hz: 145_000_000,
      params: paramsWith("probe", {}),
    };
    const next = mergeChannelSettings(sparse, {});
    expect(next.frequency_hz).toBe(145_000_000);
    expect(next.squelch).toEqual({ mode: "off" });
  });

  it("carries a decoder's edited params and an explicit null into the patch body", () => {
    const probe: ChannelSettings = {
      frequency_hz: 14_070_000,
      params: paramsWith("probe", { baud: 45.45, shift_hz: 170 }),
    };
    const edited = paramsWith("probe", { baud: 45.45, shift_hz: 850, speed: null, invert: true });
    expect(mergeChannelSettings(probe, { params: edited })).toEqual({
      frequency_hz: 14_070_000,
      squelch: { mode: "off" },
      blanker: {},
      if_filter: {},
      params: edited,
    });
  });

  it("keeps the decoder's settings when tuning", () => {
    const probe: ChannelSettings = {
      frequency_hz: 220_352_000,
      params: paramsWith("probe", { mode: "plus", service_id: 49569 }),
    };
    const next = mergeChannelSettings(probe, { frequency_hz: 225_648_000 });
    expect(next.params).toEqual(probe.params);
    expect(next.frequency_hz).toBe(225_648_000);
  });
});

describe("channelHasAudio", () => {
  it("suppresses audio controls for a data decoder", () => {
    expect(
      channelHasAudio(descriptor({ type_id: "data", has_audio: false, emits: ["data"] })),
    ).toBe(false);
  });

  it("keeps audio for a decoder that also demodulates sound", () => {
    expect(
      channelHasAudio(
        descriptor({ type_id: "voice", has_audio: true, emits: ["text"], calls: "squelch" }),
      ),
    ).toBe(true);
  });

  it("assumes audio when the descriptor is unknown or predates the flag", () => {
    expect(channelHasAudio(undefined)).toBe(true);
    expect(channelHasAudio(descriptor({}))).toBe(true);
  });
});

describe("offsetLimitHz", () => {
  it("keeps the whole passband inside the span", () => {
    expect(offsetLimitHz(2_400_000, descriptor({ bandwidth_hz: 12_500 }))).toBe(1_193_750);
  });

  it("collapses to zero rather than negative when the channel fills the span", () => {
    expect(offsetLimitHz(100_000, descriptor({ bandwidth_hz: 150_000 }))).toBe(0);
  });

  it("leaves the field unbounded when the rate is unknown", () => {
    expect(offsetLimitHz(null, descriptor({}))).toBeNull();
    expect(offsetLimitHz(0, descriptor({}))).toBeNull();
  });

  it("falls back to a point channel when the type is unknown", () => {
    expect(offsetLimitHz(2_000_000, undefined)).toBe(1_000_000);
  });
});

describe("channelWidthHz", () => {
  it("takes the band the running decoder reports first", () => {
    expect(
      channelWidthHz(
        paramsWith("probe", { bandwidth_hz: 8_000 }),
        descriptor({ bandwidth_hz: 10_000 }),
        { low_hz: 100, high_hz: 2_800 },
      ),
    ).toBe(2_700);
  });

  it("prefers the decoder's own bandwidth setting", () => {
    expect(
      channelWidthHz(
        paramsWith("probe", { bandwidth_hz: 8_000 }),
        descriptor({ bandwidth_hz: 10_000 }),
      ),
    ).toBe(8_000);
  });

  it("ignores a bandwidth setting that is not a number", () => {
    expect(
      channelWidthHz(
        paramsWith("probe", { bandwidth_hz: "wide" }),
        descriptor({ bandwidth_hz: 10_000 }),
      ),
    ).toBe(10_000);
  });

  it("falls back to the channel type width", () => {
    expect(channelWidthHz(undefined, descriptor({ bandwidth_hz: 12_500 }))).toBe(12_500);
  });

  it("hides a missing or zero width", () => {
    expect(channelWidthHz(undefined, descriptor({ bandwidth_hz: 0 }))).toBeNull();
    expect(channelWidthHz(undefined, undefined)).toBeNull();
  });
});

describe("radioWindowHz", () => {
  it("names the edges a radio can hear a channel of this width between", () => {
    expect(radioWindowHz(145_000_000, 2_400_000, descriptor({ bandwidth_hz: 12_500 }))).toEqual({
      lowHz: 143_806_250,
      highHz: 146_193_750,
    });
  });

  it("is unknown while the radio's centre or rate is", () => {
    expect(radioWindowHz(null, 2_400_000, descriptor({}))).toBeNull();
    expect(radioWindowHz(145_000_000, null, descriptor({}))).toBeNull();
  });
});

describe("reachesHz", () => {
  const window = { lowHz: 143_806_250, highHz: 146_193_750 };

  it("holds inside the window and at its edges", () => {
    expect(reachesHz(145_000_000, window)).toBe(true);
    expect(reachesHz(window.lowHz, window)).toBe(true);
    expect(reachesHz(window.highHz, window)).toBe(true);
  });

  it("fails past either edge", () => {
    expect(reachesHz(143_000_000, window)).toBe(false);
    expect(reachesHz(147_000_000, window)).toBe(false);
  });

  it("assumes reach while there is no radio to judge against", () => {
    expect(reachesHz(1_090_000_000, null)).toBe(true);
  });
});

describe("clampOffsetHz", () => {
  it("stops a step at the edge of the span, either way", () => {
    expect(clampOffsetHz(1_200_000, 1_193_750)).toBe(1_193_750);
    expect(clampOffsetHz(-1_200_000, 1_193_750)).toBe(-1_193_750);
    expect(clampOffsetHz(-25_000, 1_193_750)).toBe(-25_000);
  });

  it("leaves the offset alone while the span is unknown", () => {
    expect(clampOffsetHz(9_000_000, null)).toBe(9_000_000);
  });
});

describe("mergeAudio", () => {
  it("widens one stage's edit over the stages beside it", () => {
    const current: AudioProcessing = { agc: "medium", denoise: { enabled: true, strength: 0.4 } };
    expect(mergeAudio(current, { auto_notch: true })).toEqual({
      agc: "medium",
      denoise: { enabled: true, strength: 0.4 },
      auto_notch: true,
    });
  });

  it("starts from an empty chain", () => {
    expect(mergeAudio({}, { agc: "fast" })).toEqual({ agc: "fast" });
  });
});

describe("notch editing", () => {
  const one = { freq_hz: 1_000, width_hz: 100 };

  it("appends at the defaults until the channel is full", () => {
    const full = Array.from({ length: AUDIO_LIMITS.maxNotches }, () => one);
    expect(withNotchAdded([])).toEqual([one]);
    expect(withNotchAdded(full)).toBeNull();
  });

  it("edits and removes by position, leaving the others alone", () => {
    const notches = [one, { freq_hz: 2_000, width_hz: 50 }];
    expect(withNotchAt(notches, 1, { freq_hz: 2_500 })).toEqual([
      one,
      { freq_hz: 2_500, width_hz: 50 },
    ]);
    expect(withNotchRemoved(notches, 0)).toEqual([{ freq_hz: 2_000, width_hz: 50 }]);
  });
});

describe("audioChainActive", () => {
  it("is false for an absent or all-off chain", () => {
    expect(audioChainActive(undefined)).toBe(false);
    expect(audioChainActive({})).toBe(false);
    expect(audioChainActive({ agc: "off", notches: [], denoise: { enabled: false } })).toBe(false);
  });

  it("is true as soon as any one stage is doing something", () => {
    expect(audioChainActive({ agc: "slow" })).toBe(true);
    expect(audioChainActive({ auto_notch: true })).toBe(true);
    expect(audioChainActive({ denoise: { enabled: true } })).toBe(true);
    expect(audioChainActive({ filter: { enabled: true } })).toBe(true);
    expect(audioChainActive({ click_removal: { enabled: true } })).toBe(true);
    expect(audioChainActive({ notches: [{ freq_hz: 1_000, width_hz: 100 }] })).toBe(true);
  });
});

describe("offsetForFrequencyHz", () => {
  it("turns an absolute frequency into an offset from the tuned center", () => {
    expect(offsetForFrequencyHz(433_920_000, 433_000_000, 1_000_000)).toBe(920_000);
    expect(offsetForFrequencyHz(432_500_000, 433_000_000, 1_000_000)).toBe(-500_000);
    expect(offsetForFrequencyHz(433_000_000, 433_000_000, null)).toBe(0);
  });

  it("rounds to whole hertz", () => {
    expect(offsetForFrequencyHz(433_920_000.4, 433_000_000, null)).toBe(920_000);
  });

  it("refuses a frequency the current span cannot reach", () => {
    expect(offsetForFrequencyHz(435_000_000, 433_000_000, 1_000_000)).toBeNull();
    expect(offsetForFrequencyHz(435_000_000, 433_000_000, null)).toBe(2_000_000);
  });

  it("has no answer without finite inputs", () => {
    expect(offsetForFrequencyHz(Number.NaN, 433_000_000, null)).toBeNull();
    expect(offsetForFrequencyHz(433_000_000, Number.POSITIVE_INFINITY, null)).toBeNull();
  });
});

describe("squelchMode", () => {
  it("reads the gate state off the tagged value, off when absent", () => {
    expect(squelchMode(undefined)).toBe("off");
    expect(squelchMode({ mode: "off" })).toBe("off");
    expect(squelchMode({ mode: "manual", level_db: -60 })).toBe("manual");
    expect(squelchMode({ mode: "auto", margin_db: 8 })).toBe("auto");
    expect(squelchLevelDb({ mode: "manual", level_db: -60 })).toBe(-60);
    expect(squelchLevelDb({ mode: "auto", margin_db: 8 })).toBeNull();
    expect(squelchMarginDb({ mode: "auto", margin_db: 8 })).toBe(8);
    expect(squelchMarginDb(undefined)).toBeNull();
  });
});

describe("squelchAt", () => {
  const held = { levelDb: -55, marginDb: 12 };

  it("builds each mode from the values held for it", () => {
    expect(squelchAt("off", held)).toEqual({ mode: "off" });
    expect(squelchAt("manual", held)).toEqual({ mode: "manual", level_db: -55 });
    expect(squelchAt("auto", held)).toEqual({ mode: "auto", margin_db: 12 });
  });
});

describe("nudgedSquelch", () => {
  it("moves a manual level and clamps it to the slider range", () => {
    expect(nudgedSquelch({ mode: "manual", level_db: -60 }, 2)).toEqual({
      mode: "manual",
      level_db: -58,
    });
    expect(nudgedSquelch({ mode: "manual", level_db: -1 }, 2)).toEqual({
      mode: "manual",
      level_db: 0,
    });
  });

  it("opens a gate that was off at the default level", () => {
    expect(nudgedSquelch(undefined, -2)).toEqual({ mode: "manual", level_db: -62 });
    expect(nudgedSquelch({ mode: "off" }, 2)).toEqual({ mode: "manual", level_db: -58 });
  });

  it("moves the margin instead when the gate tracks the noise floor", () => {
    expect(nudgedSquelch({ mode: "auto", margin_db: 8 }, 2)).toEqual({
      mode: "auto",
      margin_db: 10,
    });
    expect(nudgedSquelch({ mode: "auto", margin_db: 39 }, 2)).toEqual({
      mode: "auto",
      margin_db: 40,
    });
  });
});

describe("the audio seeds and bounds", () => {
  it("are the ones the server publishes", () => {
    expect(AUDIO_DEFAULTS).toEqual({
      blankerThreshold: wire.audio.blanker_threshold_seed,
      clickThreshold: wire.audio.click_threshold_seed,
      squelchAutoMarginDb: wire.audio.squelch_auto_margin_seed_db,
      denoiseStrength: wire.audio.denoise_strength_seed,
      filterLowHz: wire.audio.filter_low_seed_hz,
      filterHighHz: wire.audio.filter_high_seed_hz,
      notchFreqHz: wire.audio.notch_freq_seed_hz,
      notchWidthHz: wire.audio.notch_width_seed_hz,
    });
    expect(AUDIO_LIMITS).toEqual({
      maxNotches: wire.audio.notches,
      blankerThreshold: wire.audio.blanker_threshold,
      clickThreshold: wire.audio.click_threshold,
      squelchAutoMarginDb: wire.audio.squelch_auto_margin_db,
      toneHz: wire.audio.tone_hz,
      notchWidthHz: wire.audio.notch_width_hz,
    });
  });
});

describe("IF filter", () => {
  const WFM_HZ = 200_000;

  it("shows the decoder's own edges until one is moved", () => {
    expect(ifEdgeHz({}, "low_hz", WFM_HZ)).toBe(-100_000);
    const cut = withIfEdge({}, "high_hz", 40_000, WFM_HZ);
    expect(ifEdgeHz(cut, "high_hz", WFM_HZ)).toBe(40_000);
    expect(withIfEdge(cut, "high_hz", 100_000, WFM_HZ).high_hz).toBeNull();
  });

  it("adds notches at the channel up to the wire limit", () => {
    let filter = withIfNotchAdded({}, 99_900_000);
    expect(filter?.notches).toEqual([
      { frequency_hz: 99_900_000, width_hz: wire.if_filter.notch_width_seed_hz },
    ]);
    for (let n = 1; n < IF_LIMITS.maxNotches; n++) {
      filter = withIfNotchAdded(filter ?? {}, 99_900_000);
    }
    expect(withIfNotchAdded(filter ?? {}, 99_900_000)).toBeNull();
    expect(withIfNotchRemoved(filter ?? {}, 0).notches).toHaveLength(IF_LIMITS.maxNotches - 1);
  });

  it("summarises only what it does", () => {
    expect(ifFilterSummary({ high_hz: 40_000 }, WFM_HZ)).toBe("off");
    expect(ifFilterSummary({ enabled: true }, WFM_HZ)).toBe("off");
    const notch = { frequency_hz: 99_930_000, width_hz: 2_000 };
    expect(ifFilterSummary({ enabled: true, notches: [notch] }, WFM_HZ)).toBe("1 notch");
    expect(ifFilterSummary({ enabled: true, high_hz: 40_000 }, WFM_HZ)).toMatch(
      /^−100 kHz \+40 kHz$/,
    );
  });
});
