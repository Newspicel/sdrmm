import { describe, expect, it } from "vitest";
import {
  BAND_ZONES,
  BW_MAX,
  BW_MIN,
  BW_TICKS,
  coversBand,
  FREQ_MAX,
  FREQ_MIN,
  fmtHz,
  fmtShortRanges,
  logPos,
  matchesRadio,
  RADIOS,
  type RadioFilter,
  SPECTRUM_TICKS,
} from "./radios";

describe("radios dataset", () => {
  it("contains valid ranges, positive rates, and docs links for every radio", () => {
    expect(RADIOS.length).toBeGreaterThanOrEqual(23);
    expect(RADIOS.some((r) => r.id === "aaronia-plus")).toBe(true);
    expect(RADIOS.some((r) => r.id === "aaronia-eco")).toBe(true);
    for (const radio of RADIOS) {
      expect(radio.ranges.length).toBeGreaterThan(0);
      for (const range of radio.ranges) {
        expect(range.low).toBeGreaterThan(0);
        expect(range.high).toBeGreaterThan(range.low);
      }
      expect(radio.maxRateHz).toBeGreaterThan(0);
      expect(radio.bandwidthHz).toBeGreaterThan(0);
      expect(radio.bandwidthHz).toBeLessThanOrEqual(BW_MAX);
      expect(radio.bits).toBeGreaterThanOrEqual(8);
      expect(radio.href).toMatch(/^\/docs\/radios\/(usb|network)#/);
      expect(fmtShortRanges(radio).length).toBeLessThanOrEqual(22);
    }
  });

  it("spaces spectrum and bandwidth ticks without collision", () => {
    for (let i = 1; i < SPECTRUM_TICKS.length; i += 1) {
      const a = SPECTRUM_TICKS[i - 1];
      const b = SPECTRUM_TICKS[i];
      if (!a || !b) continue;
      const prev = logPos(a.hz, FREQ_MIN, FREQ_MAX);
      const curr = logPos(b.hz, FREQ_MIN, FREQ_MAX);
      expect(curr - prev).toBeGreaterThanOrEqual(0.11);
    }
    for (let i = 1; i < BW_TICKS.length; i += 1) {
      const a = BW_TICKS[i - 1];
      const b = BW_TICKS[i];
      if (!a || !b) continue;
      const prev = logPos(a.hz, BW_MIN, BW_MAX);
      const curr = logPos(b.hz, BW_MIN, BW_MAX);
      expect(curr - prev).toBeGreaterThanOrEqual(0.2);
    }
    const labeledZones = BAND_ZONES.filter((z) => z.label !== "");
    for (const zone of labeledZones) {
      const width = logPos(zone.high, FREQ_MIN, FREQ_MAX) - logPos(zone.low, FREQ_MIN, FREQ_MAX);
      expect(width).toBeGreaterThanOrEqual(0.14);
    }
  });

  it("filters by capability and band accurately", () => {
    const tx = RADIOS.filter((r) => matchesRadio(r, "", new Set<RadioFilter>(["tx"])));
    expect(tx.every((r) => r.tx > 0)).toBe(true);
    expect(tx.some((r) => r.id === "hackrf")).toBe(true);
    expect(tx.some((r) => r.id === "aaronia-plus")).toBe(true);
    expect(tx.some((r) => r.id === "rtlsdr")).toBe(false);

    const hf = RADIOS.filter((r) => matchesRadio(r, "", new Set<RadioFilter>(["hf"])));
    expect(hf.some((r) => r.id === "airspy-hf")).toBe(true);
    expect(hf.some((r) => r.id === "kiwisdr")).toBe(true);
    expect(hf.some((r) => r.id === "rtlsdr")).toBe(false);

    const airspyHf = RADIOS.find((r) => r.id === "airspy-hf");
    expect(airspyHf && coversBand(airspyHf, 14e6)).toBe(true);
    expect(airspyHf && coversBand(airspyHf, 45e6)).toBe(false);
    expect(airspyHf && coversBand(airspyHf, 144e6)).toBe(true);
  });

  it("formats frequencies cleanly", () => {
    expect(fmtHz(2.4e6, "S/s")).toBe("2.4 MS/s");
    expect(fmtHz(1.766e9)).toBe("1.766 GHz");
    expect(fmtHz(192e3)).toBe("192 kHz");
  });
});
