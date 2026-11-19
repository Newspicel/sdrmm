import { describe, expect, it } from "vitest";
import type { PatchNode, RadioUse } from "../../lib/types";
import { renderFace } from "../../test/faceHarness";
import { capabilities, deviceSet, placed } from "../../test/fixtures";
import { TransmitFace, transmitNeed } from "./TransmitFace";

const TX: PatchNode = placed("tx", {
  kind: "transmit",
  data: { frequency_hz: null, level_db: -3, timeout_s: 180 },
});

describe("TransmitFace", () => {
  it("tunes like a decoder and leaves a missing wire unsaid", () => {
    const html = renderFace(TransmitFace, TX, {
      graph: { nodes: [TX], edges: [] },
      resolved: null,
    });
    expect(html).not.toContain("Wire an encoder in");
    expect(html).toContain('aria-label="Type the frequency to send on"');
    expect(html).toContain("Arm");
  });

  it("asks for a radio set to send and keeps Arm from keying one set to receive", () => {
    const html = wiredTo("receive");
    expect(html).toContain("Set the radio to send");
    const arm = html.match(/<button[^>]*title="Set the radio to send"[^>]*>/)?.[0] ?? "";
    expect(arm).toContain("disabled");
  });

  it("arms a radio set to send and receive", () => {
    const html = wiredTo("both");
    expect(html).not.toContain("Set the radio to send");
    expect(html).toContain('aria-label="Drive"');
  });

  it("names only the radio's use", () => {
    expect(transmitNeed(false, true, true)).toBeNull();
    expect(transmitNeed(true, false, true)).toBeNull();
    expect(transmitNeed(true, true, true)).toBe("Set the radio to send");
    expect(transmitNeed(true, true, false)).toBeNull();
  });
});

function wiredTo(radioUse: RadioUse): string {
  const radio = placed("radio", {
    kind: "device",
    data: { device: { backend: "virtual", key: "halfduplex" }, radio_use: radioUse },
  });
  const encoder = placed("mod", { kind: "encoder", data: { encoder_type: "nfm" } });
  const set = deviceSet({
    device: { driver: "virtual", key: "halfduplex", label: "Half-duplex 1×1 (virtual)" },
    capabilities: capabilities({ duplex: "half" }),
    radio_use: radioUse,
    uses: ["receive", "send", "both"],
  });
  return renderFace(TransmitFace, TX, {
    graph: {
      nodes: [radio, encoder, TX],
      edges: [
        { from: { node: "mod", port: "baseband" }, to: { node: "tx", port: "baseband" } },
        { from: { node: "tx", port: "transmit" }, to: { node: "radio", port: "transmit" } },
      ],
    },
    devices: new Map([["radio", set]]),
  });
}
