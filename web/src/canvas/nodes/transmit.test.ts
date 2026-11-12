import { describe, expect, it } from "vitest";
import type { ChannelDescriptor, PatchNode, TransmitStatus } from "../../lib/types";
import {
  modulatorSettings,
  modulatorTypes,
  peakText,
  tooWide,
  transmitStatusOf,
  withMicrophone,
  withModulatorSettings,
  withModulatorType,
  withTransmitSettings,
} from "./transmit";

const status = (node: string, extra: Partial<TransmitStatus> = {}): TransmitStatus => ({
  node,
  phase: "idle",
  frequency_hz: 144_800_000,
  sample_rate: 2_048_000,
  settings: { frequency_hz: null, level_db: -3, timeout_s: 180 },
  samples: 0,
  bursts: 0,
  underruns: 0,
  ...extra,
});

const descriptor = (
  type_id: string,
  extra: Partial<ChannelDescriptor> = {},
): ChannelDescriptor => ({
  type_id,
  name: type_id.toUpperCase(),
  bandwidth_hz: 12_500,
  input_rate_hz: 48_000,
  ...extra,
});

const node = (body: object): PatchNode =>
  ({ id: "n", position: { x: 0, y: 0 }, ...body }) as PatchNode;

describe("transmitStatusOf", () => {
  it("finds the transmitter this node armed on whichever radio carries it", () => {
    const sets = [{}, { transmit: status("other") }, { transmit: status("tx", { bursts: 2 }) }];
    expect(transmitStatusOf(sets, "tx")?.bursts).toBe(2);
    expect(transmitStatusOf(sets, "idle")).toBeNull();
    expect(transmitStatusOf([{ transmit: null }], "tx")).toBeNull();
  });
});

describe("transmit body edits", () => {
  it("changes one setting and keeps the rest", () => {
    const before = node({ kind: "transmit", data: { level_db: -3, timeout_s: 180 } });
    const after = withTransmitSettings(before, { frequency_hz: 145_500_000 });
    expect(after).toMatchObject({
      data: { frequency_hz: 145_500_000, level_db: -3, timeout_s: 180 },
    });
    const other = node({ kind: "scope" });
    expect(withTransmitSettings(other, { level_db: -10 })).toBe(other);
  });
});

describe("modulator", () => {
  const types = [
    descriptor("adsb"),
    descriptor("nfm", { can_transmit: true }),
    descriptor("aprs", {
      can_transmit: true,
      defaults: {
        frequency_hz: 144_800_000,
        params: { type: "aprs", settings: { mode: "afsk1200" } },
      },
    }),
  ];

  it("offers only decoders that bring a modulator", () => {
    expect(modulatorTypes(types).map((type) => type.type_id)).toEqual(["nfm", "aprs"]);
  });

  it("drops the old mode's settings when the mode changes", () => {
    const before = node({
      kind: "modulator",
      data: { channel_type: "nfm", settings: { deviation: 5 } },
    });
    expect(withModulatorType(before, "aprs")).toMatchObject({ data: { channel_type: "aprs" } });
    expect((withModulatorType(before, "aprs") as { data: object }).data).not.toHaveProperty(
      "settings",
    );
    expect(withModulatorType(before, "nfm")).toBe(before);
  });

  it("runs on its own settings, else the decoder's defaults", () => {
    expect(modulatorSettings({ mode: "g3ruh" }, types[2])).toEqual({ mode: "g3ruh" });
    expect(modulatorSettings(undefined, types[2])).toEqual({ mode: "afsk1200" });
    expect(modulatorSettings(null, undefined)).toEqual({});
    const before = node({ kind: "modulator", data: { channel_type: "aprs" } });
    expect(withModulatorSettings(before, { mode: "g3ruh" })).toMatchObject({
      data: { channel_type: "aprs", settings: { mode: "g3ruh" } },
    });
  });
});

describe("peakText", () => {
  it("shows the peak once something was sent", () => {
    expect(peakText(status("tx"))).toBe("none");
    expect(peakText(status("tx", { peak_db: -3.04 }))).toBe("-3.0 dBFS");
  });
});

describe("tooWide", () => {
  it("warns only when a measured width passes a declared channel", () => {
    expect(tooWide(status("tx", { occupied_hz: 13_000, channel_hz: 12_500 }))).toBe(true);
    expect(tooWide(status("tx", { occupied_hz: 12_500, channel_hz: 12_500 }))).toBe(false);
    expect(tooWide(status("tx", { occupied_hz: 900_000 }))).toBe(false);
    expect(tooWide(status("tx", { channel_hz: 12_500 }))).toBe(false);
  });
});

describe("withMicrophone", () => {
  it("edits a microphone and leaves any other node alone", () => {
    const mic = {
      id: "mic",
      kind: "microphone",
      data: { vox: false, threshold_db: -35, hang_ms: 600 },
      position: { x: 0, y: 0 },
    } as PatchNode;
    expect(withMicrophone(mic, { vox: true })).toEqual({
      ...mic,
      data: { vox: true, threshold_db: -35, hang_ms: 600 },
    });
    const other = { id: "s", kind: "scope", position: { x: 0, y: 0 } } as PatchNode;
    expect(withMicrophone(other, { vox: true })).toBe(other);
  });
});
