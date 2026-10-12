import { describe, expect, it } from "vitest";
import { ismStation, ismSummary } from "./ism";
import type { IsmMessage } from "./types";

const tower: IsmMessage = {
  model: "Acurite-Tower",
  id: "12053",
  channel: "C",
  temperature_c: 26.7,
  humidity_pct: 74,
  battery_ok: true,
  modulation: "ook",
  coding: "pwm",
  integrity: "checksum",
  bits: 56,
  raw: "2f1544ca12d4a6",
  snr_db: 21.5,
};

describe("ism", () => {
  it("names a sensor by model and id", () => {
    expect(ismStation(tower)).toBe("Acurite-Tower 12053");
  });

  it("summarises like the server does", () => {
    expect(ismSummary(tower)).toBe("Acurite-Tower 12053 · ch C · 26.7 °C · 74%");
    expect(ismSummary({ ...tower, battery_ok: false })).toMatch(/battery low$/);
  });

  it("gives an unknown code its length", () => {
    const unknown = { ...tower, model: "Unknown", id: "a5f0c3", channel: null, bits: 24 };
    expect(ismSummary({ ...unknown, temperature_c: null, humidity_pct: null })).toBe(
      "Unknown a5f0c3 · 24 bits",
    );
  });

  it("reports door contacts and remote commands", () => {
    const door = {
      ...tower,
      channel: null,
      temperature_c: null,
      humidity_pct: null,
      contact_open: true,
      tamper: true,
      alarm: false,
    };
    expect(ismSummary(door)).toBe("Acurite-Tower 12053 · open · tamper");
  });
});
