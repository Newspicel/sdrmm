import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ListenerRegistry } from "./listeners";
import { registerMediaLatency } from "./mediaLatency";
import { PortHub } from "./ports";
import type { ClientCommand } from "./types";
import { VideoHub, type VideoInput } from "./video";

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
    started: (streamId: number, source: string, node = "video") =>
      registry.emit("event", {
        type: "PortStream",
        data: { stream_id: streamId, node, port: "video", source, kind: "video" },
      }),
    stopped: (streamId: number) =>
      registry.emit("event", {
        type: "StreamStopped",
        data: { stream_id: streamId, kind: "video" },
      }),
    push: (streamId: number, width = 4, height = 2) =>
      registry.emit("video", {
        streamId,
        seq: 0,
        timestamp: 0n,
        width,
        height,
        format: "gray",
        pixels: new Uint8Array(width * height),
      }),
    reconnect: () => registry.emit("status", true),
    attached: () => registry.count("video") > 0,
  };
}

function bench() {
  const fake = fakeSocket();
  const ports = new PortHub();
  ports.attach(fake.socket);
  const hub = new VideoHub(ports);
  hub.attach(fake.socket);
  return { fake, hub };
}

const input = (source: string, node = "video"): VideoInput => ({ node, port: "video", source });

const subscribes = (sent: ClientCommand[]) => sent.filter((c) => c.type === "SubscribePort");
const unsubscribes = (sent: ClientCommand[]) => sent.filter((c) => c.type === "UnsubscribePort");

const waitOutGrace = () => vi.advanceTimersByTime(60_000);

describe("VideoHub", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("paces pictures against the audio of the same producer and cancels stopped pictures", () => {
    const { fake, hub } = bench();
    const remove = registerMediaLatency("atv", () => 80);
    const shown: number[] = [];
    hub.subscribe(input("atv"), (frame) => shown.push(frame.width));
    fake.started(0x8000, "atv");
    fake.push(0x8000, 160);
    expect(shown).toEqual([]);
    vi.advanceTimersByTime(79);
    expect(shown).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(shown).toEqual([160]);
    fake.push(0x8000, 320);
    fake.stopped(0x8000);
    vi.advanceTimersByTime(100);
    expect(shown).toEqual([160]);
    remove();
    hub.detach();
  });

  it("asks for the port once for every watcher and lets go after the last", () => {
    const { fake, hub } = bench();
    const first = hub.subscribe(input("atv"), () => {});
    const second = hub.subscribe(input("sstv"), () => {});
    expect(subscribes(fake.sent)).toHaveLength(1);
    first();
    waitOutGrace();
    expect(unsubscribes(fake.sent)).toHaveLength(0);
    second();
    waitOutGrace();
    expect(unsubscribes(fake.sent)).toHaveLength(1);
  });

  it("remounting inside the grace keeps the stream", () => {
    const { fake, hub } = bench();
    const stop = hub.subscribe(input("atv"), () => {});
    stop();
    vi.advanceTimersByTime(100);
    hub.subscribe(input("atv"), () => {});
    waitOutGrace();
    expect(subscribes(fake.sent)).toHaveLength(1);
    expect(unsubscribes(fake.sent)).toHaveLength(0);
  });

  it("routes each picture to the input its stream names", () => {
    const { fake, hub } = bench();
    const one: number[] = [];
    const two: number[] = [];
    hub.subscribe(input("atv"), (frame) => one.push(frame.width));
    hub.subscribe(input("sstv"), (frame) => two.push(frame.width));
    fake.started(0x8000, "atv");
    fake.started(0x8001, "sstv");
    fake.push(0x8000, 104);
    fake.push(0x8001, 164);
    fake.push(0x9999, 8);
    expect(one).toEqual([104]);
    expect(two).toEqual([164]);
  });

  it("keeps the last picture so a remounted face is not blank", () => {
    const { fake, hub } = bench();
    hub.subscribe(input("atv"), () => {});
    fake.started(0x8000, "atv");
    expect(hub.latest(input("atv"))).toBeNull();
    fake.push(0x8000, 104, 576);
    expect(hub.latest(input("atv"))?.height).toBe(576);
    expect(hub.latest(input("other"))).toBeNull();
  });

  it("asks again after a reconnect and drops the old ids", () => {
    const { fake, hub } = bench();
    const after: number[] = [];
    hub.subscribe(input("atv"), (frame) => after.push(frame.width));
    fake.started(0x8000, "atv");
    fake.reconnect();
    fake.push(0x8000);
    expect(after).toEqual([]);
    expect(subscribes(fake.sent)).toHaveLength(2);
    fake.started(0x8000, "atv");
    fake.push(0x8000);
    expect(after).toHaveLength(1);
  });

  it("detaches from the socket it leaves", () => {
    const { fake, hub } = bench();
    expect(fake.attached()).toBe(true);
    hub.detach();
    expect(fake.attached()).toBe(false);
  });

  it("lets go while the page is hidden and asks again when it shows", () => {
    const { fake, hub } = bench();
    hub.subscribe(input("atv"), () => {});
    fake.sent.length = 0;
    hub.setVisible(false);
    expect(fake.sent).toEqual([
      { type: "UnsubscribePort", data: { node: "video", port: "video" } },
    ]);
    fake.sent.length = 0;
    hub.setVisible(true);
    expect(fake.sent).toEqual([{ type: "SubscribePort", data: { node: "video", port: "video" } }]);
  });
});

describe("VideoHub pending frames", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("counts the frames it throws away so a face can show them", () => {
    const { fake, hub } = bench();
    const remove = registerMediaLatency("sstv", () => 5_000);
    const scope = input("sstv");
    fake.started(1, "sstv");
    hub.subscribe(scope, () => {});
    expect(hub.droppedFrames(scope)).toBe(0);

    const drops: number[] = [];
    const stop = hub.onDrop(() => drops.push(hub.droppedFrames(scope)));
    for (let frame = 0; frame < 40; frame++) {
      fake.push(1);
    }
    stop();
    remove();

    expect(hub.droppedFrames(scope)).toBe(8);
    expect(drops).toEqual([1, 2, 3, 4, 5, 6, 7, 8]);
  });

  it("holds no more than its byte budget of pictures", () => {
    const { fake, hub } = bench();
    const remove = registerMediaLatency("atv", () => 5_000);
    const scope = input("atv");
    fake.started(2, "atv");
    hub.subscribe(scope, () => {});
    for (let frame = 0; frame < 8; frame++) {
      fake.push(2, 2_048, 2_048);
    }
    remove();

    expect(hub.droppedFrames(scope)).toBe(4);
  });
});
