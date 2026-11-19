import { describe, expect, it } from "vitest";
import { renderFace } from "../../test/faceHarness";
import { deviceSet, placed } from "../../test/fixtures";
import { ChannelFace } from "./ChannelFace";

const RADIO = placed("radio", {
  kind: "device",
  data: { device: { backend: "virtual", key: "band" }, radio_use: "send" },
});
const VOICE = placed("voice", { kind: "channel", data: { channel_type: "nfm" } });

describe("ChannelFace on a radio set to send", () => {
  it("says so as a hint, not a fault", () => {
    const html = renderFace(ChannelFace, VOICE, {
      graph: {
        nodes: [RADIO, VOICE],
        edges: [{ from: { node: "radio", port: "iq" }, to: { node: "voice", port: "iq" } }],
      },
      devices: new Map([
        [
          "radio",
          deviceSet({
            device: { driver: "virtual", key: "band", label: "Test band" },
            radio_use: "send",
            activity: "idle",
          }),
        ],
      ]),
    });
    expect(html).toContain('text-ink-faint">Radio set to send only</span>');
    expect(html).toContain('title="Radio set to send only">send only</span>');
  });
});
