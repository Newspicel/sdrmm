import { describe, expect, it } from "vitest";
import { ListenerRegistry } from "./listeners";
import { PortHub, producerOf, sourceKey, streamSource } from "./ports";
import type { ClientCommand } from "./types";

function bench() {
  const sent: ClientCommand[] = [];
  const registry = new ListenerRegistry();
  const hub = new PortHub();
  hub.attach({ send: (command) => sent.push(command), on: registry.on.bind(registry) });
  return { sent, registry, hub };
}

describe("PortHub", () => {
  it("asks once per port, sends new params, and lets go after the last holder", () => {
    const { sent, hub } = bench();
    const first = hub.hold("scope", "iq", { fps: 30, bins: 1024 });
    const second = hub.hold("scope", "iq", { fps: 30, bins: 4096 });
    first();
    first();
    expect(sent).toEqual([
      {
        type: "SubscribePort",
        data: { node: "scope", port: "iq", params: { fps: 30, bins: 1024 } },
      },
      {
        type: "SubscribePort",
        data: { node: "scope", port: "iq", params: { fps: 30, bins: 4096 } },
      },
    ]);
    second();
    expect(sent.at(-1)).toEqual({ type: "UnsubscribePort", data: { node: "scope", port: "iq" } });
  });

  it("knows each stream by its id, and the reason a port is refused", () => {
    const { registry, hub } = bench();
    hub.hold("spk", "audio");
    registry.emit("event", {
      type: "PortRefused",
      data: { node: "spk", port: "audio", reason: "Not wired" },
    });
    expect(hub.refusalOf("spk", "audio")).toBe("Not wired");
    registry.emit("event", {
      type: "PortStream",
      data: { stream_id: 9, node: "spk", port: "audio", source: "voice", kind: "audio" },
    });
    expect(hub.refusalOf("spk", "audio")).toBeNull();
    expect(hub.streamOf(9)?.source).toBe("voice");
    registry.emit("event", { type: "StreamStopped", data: { stream_id: 9, kind: "video" } });
    expect(hub.streamsOf("spk", "audio")).toHaveLength(1);
    registry.emit("event", { type: "StreamStopped", data: { stream_id: 9, kind: "audio" } });
    expect(hub.streamsOf("spk", "audio")).toEqual([]);
  });

  it("asks again for every held port after a reconnect and forgets old ids", () => {
    const { sent, registry, hub } = bench();
    hub.hold("spk", "audio");
    registry.emit("event", {
      type: "PortStream",
      data: { stream_id: 9, node: "spk", port: "audio", source: "voice", kind: "audio" },
    });
    registry.emit("status", true);
    expect(hub.streamOf(9)).toBeUndefined();
    expect(sent).toHaveLength(2);
  });

  it("keys a stream by its source, fx chain and follower", () => {
    const { registry, hub } = bench();
    hub.hold("spk", "audio");
    registry.emit("event", {
      type: "PortStream",
      data: {
        stream_id: 9,
        node: "spk",
        port: "audio",
        source: "trunk",
        via: ["fx"],
        index: 1,
        kind: "audio",
      },
    });
    expect(hub.streamOf(9)?.source).toBe(streamSource("trunk", ["fx"], 1));
    expect(streamSource("trunk", [], 0)).toBe("trunk");
  });

  it("names the producer behind a stream key", () => {
    expect(producerOf(sourceKey("spk", "audio", streamSource("voice", ["fx"], 1)))).toBe("voice");
    expect(producerOf(sourceKey("video", "video", "atv"))).toBe("atv");
  });
});
