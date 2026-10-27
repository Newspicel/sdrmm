import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { IqFrame, SymbolFrame } from "./frame";
import { ListenerRegistry } from "./listeners";
import { PortHub } from "./ports";
import { PortTaps, RELEASE_GRACE_MS, type TapKind } from "./portTaps";
import type { ClientCommand, StreamKind } from "./types";

function fakeSocket() {
  const sent: ClientCommand[] = [];
  const registry = new ListenerRegistry();
  const socket = {
    send: (command: ClientCommand) => sent.push(command),
    on: registry.on.bind(registry),
  };
  return {
    socket,
    sent,
    started: (streamId: number, node: string, kind: StreamKind = "iq") =>
      registry.emit("event", {
        type: "PortStream",
        data: { stream_id: streamId, node, port: "baseband", source: "voice", kind },
      }),
    stopped: (streamId: number) =>
      registry.emit("event", { type: "StreamStopped", data: { stream_id: streamId, kind: "iq" } }),
    iq: (streamId: number, samples: readonly number[] = [1, 0]) =>
      registry.emit("iq", {
        streamId,
        seq: 0,
        timestamp: 0n,
        sampleRate: 24_000,
        centerHz: 145.8e6,
        samples: Float32Array.from(samples),
      }),
    symbols: (streamId: number) =>
      registry.emit("symbols", {
        streamId,
        seq: 0,
        timestamp: 0n,
        plane: "iq",
        symbolRate: 4_800,
        reference: new Float32Array(0),
        symbols: new Float32Array(0),
      } as unknown as SymbolFrame),
    reconnect: () => registry.emit("status", true),
  };
}

function bench() {
  const fake = fakeSocket();
  const ports = new PortHub();
  ports.attach(fake.socket);
  const iq = new PortTaps("iq", ports);
  const symbols = new PortTaps("symbols", ports);
  iq.attach(fake.socket);
  symbols.attach(fake.socket);
  return { fake, iq, symbols };
}

const SUBSCRIBE: ClientCommand = {
  type: "SubscribePort",
  data: { node: "scope", port: "baseband" },
};
const UNSUBSCRIBE: ClientCommand = {
  type: "UnsubscribePort",
  data: { node: "scope", port: "baseband" },
};

describe("PortTaps", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("asks for a port once however many faces and kinds watch it", () => {
    const { fake, iq, symbols } = bench();
    iq.subscribe("scope", "baseband", () => {});
    iq.subscribe("scope", "baseband", () => {});
    symbols.subscribe("scope", "baseband", () => {});
    expect(fake.sent).toEqual([SUBSCRIBE]);
  });

  it("routes each frame by the kind of the stream the server named", () => {
    const { fake, iq, symbols } = bench();
    const bursts: IqFrame[] = [];
    const blocks: SymbolFrame[] = [];
    iq.subscribe("scope", "baseband", (frame) => bursts.push(frame));
    symbols.subscribe("scope", "baseband", (frame) => blocks.push(frame));
    fake.started(40, "scope", "iq");
    fake.started(41, "scope", "symbols");
    fake.iq(40);
    fake.symbols(41);
    fake.iq(41);
    fake.iq(99);
    expect(bursts.map((frame) => frame.streamId)).toEqual([40]);
    expect(blocks.map((frame) => frame.streamId)).toEqual([41]);
  });

  it("drops frames of a stream the server stopped", () => {
    const { fake, iq } = bench();
    const bursts: IqFrame[] = [];
    iq.subscribe("scope", "baseband", (frame) => bursts.push(frame));
    fake.started(40, "scope");
    fake.stopped(40);
    fake.iq(40);
    expect(bursts).toEqual([]);
  });

  it("hands a late face the newest burst", () => {
    const { fake, iq } = bench();
    iq.subscribe("scope", "baseband", () => {});
    fake.started(40, "scope");
    fake.iq(40, [1, 0]);
    fake.iq(40, [0, 1]);
    expect([...(iq.latest("scope", "baseband")?.samples ?? [])]).toEqual([0, 1]);
  });

  it("lets go a grace period after the last watcher, and keeps it on a quick return", () => {
    const { fake, iq } = bench();
    const stop = iq.subscribe("scope", "baseband", () => {});
    stop();
    vi.advanceTimersByTime(RELEASE_GRACE_MS / 2);
    iq.subscribe("scope", "baseband", () => {});
    vi.advanceTimersByTime(RELEASE_GRACE_MS);
    expect(fake.sent).toEqual([SUBSCRIBE]);
    const { fake: other, iq: hub } = bench();
    hub.subscribe("scope", "baseband", () => {})();
    vi.advanceTimersByTime(RELEASE_GRACE_MS);
    expect(other.sent).toEqual([SUBSCRIBE, UNSUBSCRIBE]);
  });

  it("re-asks for every held port after a reconnect", () => {
    const { fake, iq } = bench();
    iq.subscribe("scope", "baseband", () => {});
    fake.reconnect();
    expect(fake.sent).toEqual([SUBSCRIBE, SUBSCRIBE]);
  });

  it("lets go while the page is hidden and asks again when it shows", () => {
    const { fake, iq } = bench();
    iq.subscribe("scope", "baseband", () => {});
    iq.setVisible(false);
    iq.setVisible(true);
    expect(fake.sent).toEqual([SUBSCRIBE, UNSUBSCRIBE, SUBSCRIBE]);
  });
});

describe("the tapped kinds", () => {
  const TAPPED = ["iq", "symbols"] as const satisfies readonly TapKind[];

  it("are stream kinds a port can carry", () => {
    const kinds: readonly StreamKind[] = TAPPED;
    expect(kinds).toEqual(["iq", "symbols"]);
  });
});
