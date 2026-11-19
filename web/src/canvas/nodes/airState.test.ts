import { describe, expect, it } from "vitest";
import type { PatchNode, ResolvedPatch, TransmitStatus } from "../../lib/types";
import { airOf, transmitterOf } from "./airState";

const status = (phase: TransmitStatus["phase"]): TransmitStatus => ({
  node: "tx",
  phase,
  frequency_hz: 144_800_000,
  sample_rate: 2_048_000,
  settings: { frequency_hz: null, level_db: -3, timeout_s: 180 },
  samples: 0,
  bursts: 0,
  underruns: 0,
});

const NODES = [
  { id: "enc", kind: "encoder", position: { x: 0, y: 0 }, data: { encoder_type: "nfm" } },
  { id: "tx", kind: "transmit", position: { x: 0, y: 0 }, data: {} },
] as unknown as PatchNode[];

const PATCH = {
  nodes: [
    {
      id: "enc",
      ports: [
        {
          name: "baseband",
          direction: "out",
          reaches: [{ port: "baseband", sink: "tx" }],
        },
      ],
    },
  ],
} as unknown as ResolvedPatch;

describe("airOf", () => {
  it("names the transmission only while armed", () => {
    expect(airOf(null)).toBeNull();
    expect(airOf(status("idle"))).toBe("armed");
    expect(airOf(status("on_air"))).toBe("on air");
  });
});

describe("transmitterOf", () => {
  it("finds the transmitter an encoder's baseband reaches", () => {
    expect(transmitterOf(PATCH, NODES, "enc")).toBe("tx");
    expect(transmitterOf(PATCH, NODES, "tx")).toBeNull();
    expect(transmitterOf(null, NODES, "enc")).toBeNull();
  });
});
