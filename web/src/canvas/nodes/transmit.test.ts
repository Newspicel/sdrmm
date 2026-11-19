import { describe, expect, it } from "vitest";
import type { EncoderDescriptor, PatchNode, TransmitStatus } from "../../lib/types";
import {
  encoderSettings,
  tooWide,
  transmitStatusOf,
  withEncoderSettings,
  withMicrophone,
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

describe("encoder settings", () => {
  const aprs: EncoderDescriptor = {
    type_id: "aprs",
    name: "APRS",
    rate_hz: 48_000,
    bandwidth_hz: 12_500,
    input: "message",
    defaults: { mode: "afsk1200" },
  };

  it("runs on its own settings, else the encoder's defaults", () => {
    expect(encoderSettings({ mode: "g3ruh" }, aprs)).toEqual({ mode: "g3ruh" });
    expect(encoderSettings(undefined, aprs)).toEqual({ mode: "afsk1200" });
    expect(encoderSettings(null, undefined)).toEqual({});
    const before = node({ kind: "encoder", data: { encoder_type: "aprs" } });
    expect(withEncoderSettings(before, { mode: "g3ruh" })).toMatchObject({
      data: { encoder_type: "aprs", settings: { mode: "g3ruh" } },
    });
    const other = node({ kind: "scope" });
    expect(withEncoderSettings(other, { mode: "g3ruh" })).toBe(other);
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
