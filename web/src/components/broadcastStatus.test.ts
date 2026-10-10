import { describe, expect, it } from "vitest";
import { broadcastFacts, broadcastTitle } from "./broadcastStatus";

describe("broadcastFacts", () => {
  it("shows audio failures independently of radio lock", () => {
    const facts = broadcastFacts({
      system: "dab",
      locked: true,
      snr_db: 20,
      frequency_error_hz: 0,
      audio_frames_ok: 41,
      audio_frames_bad: 2,
      audio_error: "Broadcast audio input queue overflow",
    });
    expect(Object.fromEntries(facts)).toMatchObject({
      "Audio frames": "41",
      "Audio failures": "2",
      "Audio error": "Broadcast audio input queue overflow",
    });
  });

  it("names the detected DAB transmission mode", () => {
    const facts = broadcastFacts({
      system: "dab_plus",
      locked: true,
      snr_db: 12,
      frequency_error_hz: 24_480,
      transmission_mode: "iii",
    });
    expect(Object.fromEntries(facts)).toMatchObject({
      System: "DAB+",
      Mode: "III",
      "Frequency error": "+24480 Hz",
    });
  });

  it("does not invent multiplex metadata", () => {
    const facts = broadcastFacts({
      system: "dvb_s2",
      locked: true,
      snr_db: 18.25,
      frequency_error_hz: -32.4,
      symbol_rate: 333_000,
    });
    expect(Object.fromEntries(facts)).toEqual({
      System: "DVB-S2",
      "Frequency error": "-32 Hz",
      "Symbol rate": "333000 Bd",
    });
  });

  it("names the superframe format, Walsh rows and scrambling codes", () => {
    const facts = broadcastFacts({
      system: "dvb_s2",
      locked: true,
      snr_db: 12,
      frequency_error_hz: 0,
      superframe: { format: 4, sosf: 37, pilot: 9, trailer: 50, reference: 7, payload: 9 },
    });
    expect(Object.fromEntries(facts)).toMatchObject({
      Superframe: "format 4, WH 37/9/50, codes 7/9",
    });
  });

  it("hides the frequency error without a lock", () => {
    expect(
      broadcastTitle({ system: "dvb_t", locked: false, snr_db: 0, frequency_error_hz: 50 }),
    ).toBe("System: DVB-T");
  });
});
