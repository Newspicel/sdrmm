import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AudioFrame } from "../frame";
import {
  type Listener,
  ListenerRegistry,
  type SocketEventKind,
  type Unsubscribe,
} from "../listeners";
import { PortHub } from "../ports";
import type { ClientCommand, ServerEvent, StreamKind } from "../types";
import { AudioEngine, type AudioSink, type AudioTap, type SinkFactory } from "./engine";
import type { WorkletReport } from "./worklet";

class FakeSocket {
  sent: ClientCommand[] = [];
  private connected = true;
  private readonly registry = new ListenerRegistry();

  send(command: ClientCommand): void {
    this.sent.push(command);
  }
  isConnected(): boolean {
    return this.connected;
  }
  on<K extends SocketEventKind>(kind: K, listener: Listener<K>): Unsubscribe {
    return this.registry.on(kind, listener);
  }

  onAudio(frame: AudioFrame): void {
    this.registry.emit("audio", frame);
  }
  emit(event: ServerEvent): void {
    this.registry.emit("event", event);
  }
  setConnected(connected: boolean): void {
    this.connected = connected;
    this.registry.emit("status", connected);
  }
}

class FakeSink implements AudioSink {
  ready?: Promise<void>;
  pushed: { opus: number[]; timestampUs: number; channels: number }[] = [];
  conceals: number[] = [];
  volume: number;
  resets = 0;
  closed = false;
  accept = true;
  report: (report: WorkletReport) => void = () => {};

  constructor(
    volume: number,
    readonly onError: (err: unknown) => void,
  ) {
    this.volume = volume;
  }
  push(opus: Uint8Array, timestampUs: number, channels: number): boolean {
    this.pushed.push({ opus: Array.from(opus), timestampUs, channels });
    return this.accept;
  }
  conceal(frames: number): void {
    this.conceals.push(frames);
  }
  setVolume(volume: number): void {
    this.volume = volume;
  }
  reset(): void {
    this.resets += 1;
  }
  close(): void {
    this.closed = true;
  }
}

function makeFactory(): { factory: SinkFactory; sinks: FakeSink[]; keys: string[] } {
  const sinks: FakeSink[] = [];
  const keys: string[] = [];
  const factory: SinkFactory = (key, volume, onError, onReport) => {
    const sink = new FakeSink(volume, onError);
    sink.report = onReport;
    sinks.push(sink);
    keys.push(key);
    return Promise.resolve(sink);
  };
  return { factory, sinks, keys };
}

function flush(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

function tap(channel: number, source = "voice"): AudioTap {
  return { node: `spk${channel}`, port: "audio", source };
}

function started(channel: number, streamId: number, source = "voice"): ServerEvent {
  return {
    type: "PortStream",
    data: { stream_id: streamId, node: `spk${channel}`, port: "audio", source, kind: "audio" },
  };
}

function subscribe(channel: number): ClientCommand {
  return { type: "SubscribePort", data: { node: `spk${channel}`, port: "audio" } };
}

function unsubscribe(channel: number): ClientCommand {
  return { type: "UnsubscribePort", data: { node: `spk${channel}`, port: "audio" } };
}

function stopped(streamId: number, kind: StreamKind = "audio"): ServerEvent {
  return { type: "StreamStopped", data: { stream_id: streamId, kind } };
}

function audioFrame(
  streamId: number,
  timestamp: bigint,
  bytes: number[],
  chLayout = 1,
): AudioFrame {
  return { streamId, seq: 0, timestamp, chLayout, opus: Uint8Array.from(bytes) };
}

describe("AudioEngine", () => {
  let socket: FakeSocket;
  let sinks: FakeSink[];
  let engine: AudioEngine;

  beforeEach(() => {
    socket = new FakeSocket();
    const made = makeFactory();
    sinks = made.sinks;
    const ports = new PortHub();
    ports.attach(socket);
    engine = new AudioEngine(made.factory, ports);
    engine.attach(socket);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  function fresh(factory: SinkFactory): AudioEngine {
    const ports = new PortHub();
    ports.attach(socket);
    const made = new AudioEngine(factory, ports);
    made.attach(socket);
    return made;
  }

  it("start holds the port once the sink is ready, and PortStream binds the stream id", async () => {
    engine.start(tap(2));
    expect(engine.isPlaying(tap(2))).toBe(false);
    expect(engine.isPending(tap(2))).toBe(true);
    await flush();
    expect(socket.sent).toEqual([subscribe(2)]);

    socket.emit(started(2, 9));
    expect(engine.isPlaying(tap(2))).toBe(true);
    expect(engine.isPending(tap(2))).toBe(false);
  });

  it("waits for audio output before subscribing and resumes each waiting channel once", async () => {
    engine.setOutputRunning(false);
    engine.start(tap(2));
    engine.start(tap(3));
    await flush();
    expect(sinks).toHaveLength(2);
    expect(socket.sent).toEqual([]);
    engine.setOutputRunning(true);
    engine.setOutputRunning(true);
    expect(socket.sent).toEqual([subscribe(2), subscribe(3)]);
    socket.emit(started(2, 10));
    socket.onAudio(audioFrame(10, 0n, [1]));
    expect(sinks[0]?.pushed).toHaveLength(1);
  });

  it("reroute rebuilds each playing channel on a fresh sink and resubscribes", async () => {
    engine.start(tap(2));
    await flush();
    socket.emit(started(2, 9));
    socket.sent = [];

    engine.rerouteOutput();
    expect(sinks[0]?.closed).toBe(true);
    expect(engine.isPending(tap(2))).toBe(true);
    await flush();

    expect(sinks).toHaveLength(2);
    expect(socket.sent).toEqual([unsubscribe(2), subscribe(2)]);
    socket.emit(started(2, 11));
    socket.onAudio(audioFrame(11, 0n, [1]));
    expect(sinks[1]?.pushed).toHaveLength(1);
  });

  it("does not subscribe a cancelled start when delayed output becomes ready", async () => {
    engine.setOutputRunning(false);
    engine.start(tap(2));
    await flush();
    engine.stop(tap(2));
    engine.setOutputRunning(true);
    expect(socket.sent).toEqual([]);
    expect(sinks[0]?.closed).toBe(true);
  });

  it("waits for the first render callback even when the output state is running", async () => {
    let activate: (() => void) | undefined;
    const ready = new Promise<void>((resolve) => {
      activate = resolve;
    });
    engine.start(tap(2));
    const sink = sinks[0];
    if (!sink || !activate) throw new Error("sink readiness missing");
    sink.ready = ready;
    await flush();
    engine.setOutputRunning(false);
    engine.setOutputRunning(true);
    expect(socket.sent).toEqual([]);
    activate();
    await flush();
    expect(socket.sent).toEqual([subscribe(2)]);
  });

  it("cancels a sink waiting for rendering without delaying its replacement", async () => {
    let activate: (() => void) | undefined;
    const ready = new Promise<void>((resolve) => {
      activate = resolve;
    });
    engine.start(tap(2));
    const old = sinks[0];
    if (!old || !activate) throw new Error("sink readiness missing");
    old.ready = ready;
    await flush();
    engine.stop(tap(2));
    expect(old.closed).toBe(true);
    engine.start(tap(2));
    await flush();
    expect(socket.sent).toHaveLength(1);
    activate();
    await flush();
    expect(socket.sent).toHaveLength(1);
    socket.emit(started(2, 11));
    socket.onAudio(audioFrame(11, 0n, [1]));
    expect(sinks[1]?.pushed).toHaveLength(1);
  });

  it("surfaces playback readiness failures and closes the prepared sink", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    let rejectReady: ((error: unknown) => void) | undefined;
    const ready = new Promise<void>((_resolve, reject) => {
      rejectReady = reject;
    });
    engine.start(tap(2));
    const sink = sinks[0];
    if (!sink || !rejectReady) throw new Error("sink readiness missing");
    sink.ready = ready;
    await flush();
    rejectReady(new Error("audio output failed"));
    await flush();
    expect(engine.getError(tap(2))).toBe("audio output failed");
    expect(sink.closed).toBe(true);
    expect(socket.sent).toEqual([]);
  });

  it("routes frames by stream id, converting 48 kHz sample timestamps to µs", async () => {
    engine.start(tap(2));
    engine.start(tap(3));
    await flush();
    socket.emit(started(2, 10));
    socket.emit(started(3, 11));

    socket.onAudio(audioFrame(10, 48_000n, [1, 2]));
    socket.onAudio(audioFrame(11, 24_000n, [3]));
    socket.onAudio(audioFrame(99, 0n, [4]));

    expect(sinks[0]?.pushed).toEqual([{ opus: [1, 2], timestampUs: 1_000_000, channels: 1 }]);
    expect(sinks[1]?.pushed).toEqual([{ opus: [3], timestampUs: 500_000, channels: 1 }]);
  });

  it("hands each frame's channel layout to the sink without disturbing the clock", async () => {
    engine.start(tap(2));
    await flush();
    socket.emit(started(2, 10));

    socket.onAudio(audioFrame(10, 0n, [1], 1));
    socket.onAudio(audioFrame(10, 960n, [2], 2));
    socket.onAudio(audioFrame(10, 1_920n, [3], 2));

    expect(sinks[0]?.pushed).toEqual([
      { opus: [1], timestampUs: 0, channels: 1 },
      { opus: [2], timestampUs: 20_000, channels: 2 },
      { opus: [3], timestampUs: 40_000, channels: 2 },
    ]);
    expect(sinks[0]?.conceals).toEqual([]);
    expect(sinks[0]?.resets).toBe(1);
  });

  it("StreamStopped keeps the intent and waits for the port's next stream", async () => {
    engine.start(tap(2));
    await flush();
    socket.emit(started(2, 10));
    socket.sent = [];

    socket.emit(stopped(10));
    expect(engine.isPlaying(tap(2))).toBe(false);
    expect(engine.isPending(tap(2))).toBe(true);
    expect(sinks[0]?.closed).toBe(false);
    expect(socket.sent).toEqual([]);

    socket.onAudio(audioFrame(10, 0n, [1]));
    expect(sinks[0]?.pushed).toEqual([]);
    socket.emit(started(2, 11));
    socket.onAudio(audioFrame(11, 0n, [1]));
    expect(sinks[0]?.pushed).toHaveLength(1);
  });

  it("a spectrum StreamStopped never touches audio entries", async () => {
    engine.start(tap(2));
    await flush();
    socket.emit(started(2, 7));

    socket.emit(stopped(7, "spectrum"));
    expect(engine.isPlaying(tap(2))).toBe(true);
    expect(sinks[0]?.closed).toBe(false);
  });

  it("stop lets go of the port and tears down the sink", async () => {
    engine.start(tap(2));
    await flush();
    socket.emit(started(2, 10));

    engine.stop(tap(2));
    expect(engine.isPlaying(tap(2))).toBe(false);
    expect(sinks[0]?.closed).toBe(true);
    expect(socket.sent.at(-1)).toEqual(unsubscribe(2));
  });

  it("rapid Play→Stop→Play: the superseded stream's stop leaves the new stream intact", async () => {
    engine.start(tap(2));
    await flush();
    engine.stop(tap(2));
    engine.start(tap(2));
    await flush();
    expect(socket.sent).toEqual([subscribe(2), unsubscribe(2), subscribe(2)]);

    socket.emit(started(2, 0x8000));
    socket.emit(stopped(0x8000));
    expect(engine.isPending(tap(2))).toBe(true);
    socket.emit(started(2, 0x8001));
    expect(engine.isPlaying(tap(2))).toBe(true);
    expect(sinks[1]?.closed).toBe(false);

    socket.onAudio(audioFrame(0x8001, 0n, [7]));
    expect(sinks[1]?.pushed).toEqual([{ opus: [7], timestampUs: 0, channels: 1 }]);
    expect(socket.sent).toHaveLength(3);
  });

  it("socket close flips playing false, reconnect resubscribes and rebinds a fresh id", async () => {
    engine.start(tap(2));
    await flush();
    socket.emit(started(2, 10));
    socket.sent = [];
    const resetsAfterFirstBind = sinks[0]?.resets ?? 0;

    socket.setConnected(false);
    expect(engine.isPlaying(tap(2))).toBe(false);

    socket.setConnected(true);
    expect(socket.sent).toEqual([subscribe(2)]);

    socket.emit(started(2, 20));
    expect(engine.isPlaying(tap(2))).toBe(true);
    expect(sinks[0]?.resets).toBe(resetsAfterFirstBind + 1);
    socket.onAudio(audioFrame(20, 0n, [5]));
    expect(sinks[0]?.pushed).toEqual([{ opus: [5], timestampUs: 0, channels: 1 }]);
  });

  it("start while disconnected asks for the port again once connected", async () => {
    socket.setConnected(false);
    engine.start(tap(2));
    await flush();
    socket.sent = [];

    socket.setConnected(true);
    expect(socket.sent).toEqual([subscribe(2)]);
  });

  it("volume clamps, applies to the live sink, and persists across stop/start", async () => {
    engine.start(tap(2));
    await flush();
    engine.setVolume(tap(2), 0.3);
    expect(sinks[0]?.volume).toBe(0.3);
    engine.setVolume(tap(2), 1.7);
    expect(engine.getVolume(tap(2))).toBe(1);
    engine.setVolume(tap(2), 0.3);

    engine.stop(tap(2));
    engine.start(tap(2));
    await flush();
    expect(engine.getVolume(tap(2))).toBe(0.3);
    expect(sinks[1]?.volume).toBe(0.3);
  });

  it("a decode error surfaces to the store and stops the channel", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    engine.start(tap(2));
    await flush();
    socket.emit(started(2, 10));

    sinks[0]?.onError(new Error("decode failed"));
    expect(engine.getError(tap(2))).toBe("decode failed");
    expect(engine.isPlaying(tap(2))).toBe(false);
    expect(sinks[0]?.closed).toBe(true);
    expect(socket.sent.at(-1)).toEqual(unsubscribe(2));

    engine.clearError(tap(2));
    expect(engine.getError(tap(2))).toBe(null);
  });

  it("ignores retired sink failures and reports after playback restarts", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    engine.start(tap(2));
    await flush();
    socket.emit(started(2, 10));
    const retired = sinks[0];
    if (!retired) throw new Error("first sink missing");
    engine.stop(tap(2));
    engine.start(tap(2));
    await flush();
    socket.emit(started(2, 11));
    retired.report({ underruns: 4, decoderDroppedFrames: 960, trimmedFrames: 960 });
    retired.onError(new Error("late decoder failure"));
    socket.onAudio(audioFrame(11, 0n, [1]));
    expect(engine.isPlaying(tap(2))).toBe(true);
    expect(engine.getError(tap(2))).toBeNull();
    expect(engine.getLostFrames(tap(2))).toBe(0);
    expect(engine.getUnderruns(tap(2))).toBe(0);
    expect(engine.getTrimmedMs(tap(2))).toBe(0);
    expect(sinks[1]?.pushed).toHaveLength(1);
    expect(sinks[1]?.closed).toBe(false);
  });

  it("retries the current start when a superseded sink initialization rejects", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    let rejectFirst: ((error: unknown) => void) | undefined;
    const first = new Promise<AudioSink>((_resolve, reject) => {
      rejectFirst = reject;
    });
    const made = makeFactory();
    let attempts = 0;
    const restarting = fresh((...args) => {
      attempts++;
      return attempts === 1 ? first : made.factory(...args);
    });
    restarting.start(tap(2));
    restarting.stop(tap(2));
    restarting.start(tap(2));
    if (!rejectFirst) throw new Error("first initialization missing");
    rejectFirst(new Error("old initialization failed"));
    await flush();
    expect(attempts).toBe(2);
    expect(restarting.getError(tap(2))).toBeNull();
    socket.emit(started(2, 11));
    socket.onAudio(audioFrame(11, 0n, [1]));
    expect(restarting.isPlaying(tap(2))).toBe(true);
    expect(made.sinks[0]?.pushed).toHaveLength(1);
  });

  it("a sink factory failure surfaces to the store and clears intent", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const failing = fresh(() => Promise.reject(new Error("no audio")));
    failing.start(tap(2));
    await flush();
    expect(failing.getError(tap(2))).toBe("no audio");
    expect(failing.isPlaying(tap(2))).toBe(false);
    expect(failing.isPending(tap(2))).toBe(false);
    expect(socket.sent).toEqual([]);
    socket.setConnected(false);
    socket.setConnected(true);
    expect(socket.sent).toEqual([]);

    failing.start(tap(2));
    expect(failing.getError(tap(2))).toBe(null);
  });

  it("a refused port shows its reason and plays once the port is wired", async () => {
    engine.start(tap(2));
    await flush();
    socket.emit({
      type: "PortRefused",
      data: { node: "spk2", port: "audio", reason: "Not wired" },
    });
    expect(engine.getError(tap(2))).toBe("Not wired");
    expect(engine.isPending(tap(2))).toBe(true);

    socket.emit(started(2, 0x8001));
    expect(engine.getError(tap(2))).toBe(null);
    expect(engine.isPlaying(tap(2))).toBe(true);
  });

  it("suspends subscriptions and waits for a new sink before resuming", async () => {
    engine.start(tap(2));
    await flush();
    socket.emit(started(2, 10));
    expect(engine.isPlaying(tap(2))).toBe(true);

    engine.setOutputRunning(false);
    engine.setOutputRunning(false);
    expect(engine.isPlaying(tap(2))).toBe(false);
    expect(engine.isSuspended(tap(2))).toBe(true);
    expect(engine.isPending(tap(2))).toBe(false);
    expect(sinks[0]?.closed).toBe(true);
    expect(socket.sent.filter((command) => command.type === "UnsubscribePort")).toHaveLength(1);
    socket.onAudio(audioFrame(10, 0n, [1]));
    expect(sinks[0]?.pushed).toEqual([]);

    engine.setOutputRunning(true);
    expect(engine.isPlaying(tap(2))).toBe(false);
    let activate: (() => void) | undefined;
    const resumed = sinks[1];
    if (!resumed) throw new Error("replacement sink missing");
    resumed.ready = new Promise<void>((resolve) => {
      activate = resolve;
    });
    await flush();
    expect(socket.sent.filter((command) => command.type === "SubscribePort")).toHaveLength(1);
    if (!activate) throw new Error("replacement readiness missing");
    activate();
    await flush();
    socket.emit(stopped(10));
    socket.emit(started(2, 11));
    expect(engine.isPlaying(tap(2))).toBe(true);
    expect(engine.isSuspended(tap(2))).toBe(false);
    socket.onAudio(audioFrame(11, 48_000n, [2]));
    expect(sinks[1]?.pushed).toHaveLength(1);
    expect(engine.getLostFrames(tap(2))).toBe(0);
  });

  it("ignores a suspended subscription's late reply and does not resume stopped channels", async () => {
    engine.start(tap(2));
    engine.start(tap(3));
    await flush();
    engine.setOutputRunning(false);
    socket.emit(started(2, 10));
    socket.emit(started(3, 11));
    socket.emit(stopped(10));
    expect(engine.isSuspended(tap(2))).toBe(true);
    engine.stop(tap(3));
    socket.sent = [];
    engine.setOutputRunning(true);
    await flush();
    expect(socket.sent).toEqual([subscribe(2)]);
    socket.emit(started(2, 12));
    expect(engine.isPlaying(tap(2))).toBe(true);
    expect(engine.isPending(tap(3))).toBe(false);
  });

  it("refreshes a disconnected sink's readiness after output suspension", async () => {
    socket.setConnected(false);
    engine.start(tap(2));
    await flush();
    expect(sinks[0]?.closed).toBe(false);
    engine.setOutputRunning(false);
    expect(sinks[0]?.closed).toBe(true);
    socket.sent = [];
    socket.setConnected(true);
    expect(socket.sent).toEqual([]);
    expect(sinks).toHaveLength(1);
    engine.setOutputRunning(true);
    await flush();
    socket.emit(started(2, 12));
    expect(engine.isPlaying(tap(2))).toBe(true);
    expect(sinks[1]?.closed).toBe(false);
  });

  it("timestamp gaps conceal the missing audio; oversized gaps reset the sink", async () => {
    engine.start(tap(2));
    await flush();
    socket.emit(started(2, 10));
    const resetsAfterBind = sinks[0]?.resets ?? 0;

    socket.onAudio(audioFrame(10, 0n, [1]));
    socket.onAudio(audioFrame(10, 960n, [2]));
    socket.onAudio(audioFrame(10, 1_920n, [3]));
    expect(sinks[0]?.conceals).toEqual([]);

    socket.onAudio(audioFrame(10, 3_840n, [4]));
    expect(sinks[0]?.conceals).toEqual([960]);

    socket.onAudio(audioFrame(10, 34_800n, [5]));
    expect(sinks[0]?.resets).toBe(resetsAfterBind + 1);
    expect(sinks[0]?.pushed).toHaveLength(5);
  });

  it("keeps stream loss and local starvation apart, and accumulates both across sinks", async () => {
    engine.start(tap(2));
    await flush();
    socket.emit(started(2, 10));
    expect(engine.getLostFrames(tap(2))).toBe(0);
    expect(engine.getUnderruns(tap(2))).toBe(0);

    socket.onAudio(audioFrame(10, 0n, [1]));
    socket.onAudio(audioFrame(10, 960n, [2]));
    socket.onAudio(audioFrame(10, 2_880n, [3]));
    expect(engine.getLostFrames(tap(2))).toBe(960);
    expect(engine.getUnderruns(tap(2))).toBe(0);

    sinks[0]?.report({ underruns: 2 });
    sinks[0]?.report({ underruns: 3 });
    expect(engine.getUnderruns(tap(2))).toBe(3);
    expect(engine.getLostFrames(tap(2))).toBe(960);

    engine.stop(tap(2));
    engine.start(tap(2));
    await flush();
    sinks[1]?.report({ underruns: 1 });
    expect(engine.getUnderruns(tap(2))).toBe(4);
  });

  it("reports buffer and discarded audio, and clears the buffer when stopped", async () => {
    engine.start(tap(2));
    await flush();
    socket.emit(started(2, 10));
    sinks[0]?.report({
      underruns: 0,
      bufferedFrames: 2880,
      decoderDroppedFrames: 960,
      trimmedFrames: 480,
    });
    expect(engine.getBufferedMs(tap(2))).toBe(60);
    expect(engine.getTrimmedMs(tap(2))).toBe(10);
    expect(engine.getLostFrames(tap(2))).toBe(960);

    engine.stop(tap(2));
    expect(engine.getBufferedMs(tap(2))).toBe(0);
  });

  it("a source the server stops while another plays leaves the speaker", async () => {
    engine.start(tap(2));
    await flush();
    socket.emit(started(2, 10));
    socket.emit(started(2, 11, "other"));
    expect(engine.isPending(tap(2, "other"))).toBe(true);
    await flush();
    socket.sent.length = 0;

    socket.emit(stopped(10));

    expect(engine.isPlaying(tap(2))).toBe(false);
    expect(engine.isPending(tap(2))).toBe(false);
    expect(sinks[0]?.closed).toBe(true);
    expect(engine.isPlaying(tap(2, "other"))).toBe(true);
    expect(socket.sent).toEqual([]);
  });

  it("keeps two sources of one speaker apart on one port subscription", async () => {
    const made = makeFactory();
    const routed = fresh(made.factory);
    routed.start(tap(2));
    routed.start(tap(2, "voice/fx"));
    await flush();
    expect(socket.sent).toEqual([subscribe(2)]);
    expect(new Set(made.keys).size).toBe(2);

    socket.emit(started(2, 12, "voice/fx"));
    expect(routed.isPlaying(tap(2, "voice/fx"))).toBe(true);
    expect(routed.isPlaying(tap(2))).toBe(false);
  });

  it("a playing speaker plays the input a rewire brings", async () => {
    engine.start(tap(2));
    await flush();
    socket.emit(started(2, 10));
    socket.emit(stopped(10));
    socket.emit(started(2, 11, "other"));
    expect(engine.isPending(tap(2, "other"))).toBe(true);
    await flush();
    socket.emit(started(2, 11, "other"));
    expect(engine.isPlaying(tap(2, "other"))).toBe(true);
    expect(engine.isPlaying(tap(2))).toBe(false);
    expect(engine.isPending(tap(2))).toBe(false);
  });

  it("a stopped speaker starts nothing a new stream brings", async () => {
    engine.start(tap(2));
    await flush();
    socket.emit(started(2, 10));
    engine.stop(tap(2));
    socket.sent.length = 0;

    socket.emit(started(2, 11, "other"));
    expect(engine.isPending(tap(2, "other"))).toBe(false);
    expect(socket.sent).toEqual([]);
  });

  it("conceals a packet the sink refuses, so the loss cannot pass as an underrun", async () => {
    engine.start(tap(2));
    await flush();
    socket.emit(started(2, 10));
    const sink = sinks[0];

    socket.onAudio(audioFrame(10, 0n, [1]));
    socket.onAudio(audioFrame(10, 960n, [2]));
    expect(sink?.conceals).toEqual([]);

    if (sink) {
      sink.accept = false;
    }
    socket.onAudio(audioFrame(10, 1_920n, [3]));
    expect(sink?.conceals).toEqual([960]);
  });

  it("a stop/start race during sink creation still ends with a live sink", async () => {
    engine.start(tap(2));
    engine.stop(tap(2));
    engine.start(tap(2));
    await flush();
    expect(sinks[0]?.closed).toBe(true);
    expect(sinks[1]?.closed).toBe(false);
    expect(socket.sent.filter((c) => c.type === "SubscribePort")).toEqual([subscribe(2)]);
  });

  it("notifies store subscribers on state changes", async () => {
    const listener = vi.fn();
    const leave = engine.subscribe(listener);
    engine.start(tap(2));
    await flush();
    socket.emit(started(2, 10));
    expect(listener).toHaveBeenCalled();

    listener.mockClear();
    leave();
    engine.stop(tap(2));
    expect(listener).not.toHaveBeenCalled();
  });
});
