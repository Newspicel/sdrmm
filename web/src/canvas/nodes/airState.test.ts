import { describe, expect, it } from "vitest";
import type { PatchNode, ResolvedPatch, TransmitStatus } from "../../lib/types";
import { airOf, reachesArmedTransmitter, reachesListener, transmitterOf } from "./airState";

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

const reaching = (sinks: string[]) =>
  ({
    nodes: [
      {
        id: "mic",
        ports: [
          {
            name: "audio",
            direction: "out",
            reaches: sinks.map((sink) => ({ port: "audio", sink })),
          },
        ],
      },
      ...PATCH.nodes,
    ],
  }) as unknown as ResolvedPatch;

describe("reachesArmedTransmitter", () => {
  const nodes = [
    { id: "mic", kind: "microphone", position: { x: 0, y: 0 }, data: {} },
    { id: "spk", kind: "speaker", position: { x: 0, y: 0 }, data: {} },
    ...NODES,
  ] as unknown as PatchNode[];
  const armed = [{ transmit: status("idle") }];

  it("holds while an encoder it feeds drives an armed transmitter", () => {
    expect(reachesArmedTransmitter(reaching(["enc"]), nodes, armed, "mic")).toBe(true);
    expect(reachesArmedTransmitter(reaching(["spk", "enc"]), nodes, armed, "mic")).toBe(true);
  });

  it("fails while the transmitter is disarmed, unreached or reached without an encoder", () => {
    expect(reachesArmedTransmitter(reaching(["enc"]), nodes, [{ transmit: null }], "mic")).toBe(
      false,
    );
    expect(reachesArmedTransmitter(reaching(["spk"]), nodes, armed, "mic")).toBe(false);
    expect(reachesArmedTransmitter(reaching(["tx"]), nodes, armed, "mic")).toBe(false);
    expect(reachesArmedTransmitter(reaching([]), nodes, armed, "mic")).toBe(false);
    expect(reachesArmedTransmitter(null, nodes, armed, "mic")).toBe(false);
  });
});

describe("reachesListener", () => {
  const nodes = [
    { id: "mic", kind: "microphone", position: { x: 0, y: 0 }, data: {} },
    { id: "spk", kind: "speaker", position: { x: 0, y: 0 }, data: {} },
    ...NODES,
  ] as unknown as PatchNode[];
  const idle = [{ transmit: null }];

  it("holds for a speaker, or an encoder only while its transmitter is armed", () => {
    expect(reachesListener(reaching(["spk"]), nodes, idle, "mic")).toBe(true);
    expect(reachesListener(reaching(["enc"]), nodes, idle, "mic")).toBe(false);
    expect(reachesListener(reaching(["enc"]), nodes, [{ transmit: status("idle") }], "mic")).toBe(
      true,
    );
    expect(reachesListener(reaching([]), nodes, idle, "mic")).toBe(false);
  });

  it("holds for an encoder whose baseband goes to a recorder, armed or not", () => {
    const recorded = reaching(["enc"]);
    const enc = recorded.nodes.find((held) => held.id === "enc");
    const out = enc?.ports.find((port) => port.name === "baseband");
    out?.reaches?.push({ port: "baseband", sink: "rec" });
    const withRecorder = [
      ...nodes,
      { id: "rec", kind: "baseband_recorder", position: { x: 0, y: 0 }, data: {} },
    ] as unknown as PatchNode[];
    expect(reachesListener(recorded, withRecorder, idle, "mic")).toBe(true);
  });
});
