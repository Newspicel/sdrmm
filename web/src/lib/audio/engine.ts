import type { AudioFrame } from "../frame";
import type { Listener, Unsubscribe } from "../listeners";
import { type PortHub, portHub, portKey, sourceKey } from "../ports";
import { LossTracker } from "./loss";
import { MAX_GAP_FRAMES, SAMPLE_RATE, type WorkletReport } from "./worklet";

export interface AudioSink {
  readonly ready?: Promise<void>;
  push(opus: Uint8Array, timestampUs: number, channels: number): boolean;
  conceal(frames: number): void;
  setVolume(volume: number): void;
  reset(): void;
  close(): void;
}

export type SinkFactory = (
  key: string,
  volume: number,
  onError: (err: unknown) => void,
  onReport: (report: WorkletReport) => void,
) => Promise<AudioSink>;

export interface AudioSocket {
  on<K extends "audio" | "status">(kind: K, listener: Listener<K>): Unsubscribe;
}

export interface AudioTap {
  readonly node: string;
  readonly port: string;
  readonly source: string;
}

interface ChannelEntry {
  readonly tap: AudioTap;
  desired: boolean;
  hold: (() => void) | null;
  streamId: number | null;
  sink: AudioSink | null;
  sinkReady: boolean;
  sinkPending: boolean;
  generation: number;
  volume: number;
  lastError: string | null;
  loss: LossTracker;
  lost: number;
  underruns: number;
  sinkUnderruns: number;
  decoderDroppedFrames: number;
  bufferedFrames: number;
  trimmedFrames: number;
}

const US_PER_FRAME = 1_000_000 / SAMPLE_RATE;

export function tapKey(tap: AudioTap): string {
  return sourceKey(tap.node, tap.port, tap.source);
}

function keyOf(entry: ChannelEntry): string {
  return tapKey(entry.tap);
}

export class AudioEngine {
  private readonly entries = new Map<string, ChannelEntry>();
  private readonly storeListeners = new Set<() => void>();
  private outputRunning = true;
  private socket: AudioSocket | null = null;
  private unsubscribes: Unsubscribe[] = [];

  constructor(
    private readonly createSink: SinkFactory,
    private readonly ports: PortHub = portHub,
  ) {}

  readonly subscribe = (listener: () => void): (() => void) => {
    this.storeListeners.add(listener);
    return () => this.storeListeners.delete(listener);
  };

  attach(socket: AudioSocket): void {
    if (this.socket === socket) {
      return;
    }
    this.detach();
    this.socket = socket;
    this.unsubscribes = [
      socket.on("status", this.handleStatus),
      socket.on("audio", this.handleAudio),
      this.ports.subscribe(this.followStreams),
    ];
  }

  detach(): void {
    if (!this.socket) {
      return;
    }
    for (const unsubscribe of this.unsubscribes) {
      unsubscribe();
    }
    this.unsubscribes = [];
    this.socket = null;
    this.dropLiveStreams();
  }

  isPlaying(tap: AudioTap): boolean {
    const entry = this.entries.get(tapKey(tap));
    return entry !== undefined && entry.desired && entry.streamId !== null && this.outputRunning;
  }

  isPending(tap: AudioTap): boolean {
    const entry = this.entries.get(tapKey(tap));
    return entry !== undefined && entry.desired && entry.streamId === null && this.outputRunning;
  }

  isSuspended(tap: AudioTap): boolean {
    const entry = this.entries.get(tapKey(tap));
    return entry !== undefined && entry.desired && !this.outputRunning;
  }

  getError(tap: AudioTap): string | null {
    return this.entries.get(tapKey(tap))?.lastError ?? null;
  }

  clearError(tap: AudioTap): void {
    const entry = this.entries.get(tapKey(tap));
    if (entry && entry.lastError !== null) {
      entry.lastError = null;
      this.notify();
    }
  }

  getVolume(tap: AudioTap): number {
    return this.entries.get(tapKey(tap))?.volume ?? 1;
  }

  getLostFrames(tap: AudioTap): number {
    return this.entries.get(tapKey(tap))?.lost ?? 0;
  }

  getUnderruns(tap: AudioTap): number {
    return this.entries.get(tapKey(tap))?.underruns ?? 0;
  }

  setOutputRunning(running: boolean): void {
    if (this.outputRunning === running) {
      return;
    }
    this.outputRunning = running;
    for (const entry of this.entries.values()) {
      if (!entry.desired) continue;
      if (running) {
        if (entry.hold === null) this.ensureSink(entry);
      } else if (entry.hold !== null || entry.sinkReady) {
        entry.generation += 1;
        this.releaseSubscription(entry);
        this.teardown(entry);
      }
    }
    this.notify();
  }

  rerouteOutput(): void {
    for (const entry of this.entries.values()) {
      if (!entry.desired) continue;
      entry.generation += 1;
      this.releaseSubscription(entry);
      this.teardown(entry);
      if (this.outputRunning) this.ensureSink(entry);
    }
    this.notify();
  }

  start(tap: AudioTap): void {
    const entry = this.ensureEntry(tap);
    if (entry.desired) {
      return;
    }
    entry.desired = true;
    entry.lastError = null;
    entry.generation += 1;
    this.notify();
    this.ensureSink(entry);
  }

  stop(tap: AudioTap): void {
    const entry = this.entries.get(tapKey(tap));
    if (!entry || !entry.desired) {
      return;
    }
    entry.desired = false;
    entry.generation += 1;
    this.releaseSubscription(entry);
    this.teardown(entry);
    this.notify();
  }

  private releaseSubscription(entry: ChannelEntry): void {
    entry.streamId = null;
    entry.hold?.();
    entry.hold = null;
  }

  retain(live: Iterable<AudioTap>): void {
    const wired = new Map<string, AudioTap>();
    for (const tap of live) {
      wired.set(tapKey(tap), tap);
    }
    const playing = new Set<string>();
    for (const [key, entry] of this.entries) {
      if (!entry.desired) {
        continue;
      }
      playing.add(portKey(entry.tap.node, entry.tap.port));
      if (!wired.has(key)) {
        this.stop(entry.tap);
      }
    }
    for (const [key, tap] of wired) {
      if (!this.entries.has(key) && playing.has(portKey(tap.node, tap.port))) {
        this.start(tap);
      }
    }
  }

  setVolume(tap: AudioTap, volume: number): void {
    const entry = this.ensureEntry(tap);
    entry.volume = Math.min(1, Math.max(0, volume));
    entry.sink?.setVolume(entry.volume);
    this.notify();
  }

  private readonly followStreams = (): void => {
    let changed = false;
    for (const entry of this.entries.values()) {
      if (entry.hold === null) {
        continue;
      }
      const { node, port, source } = entry.tap;
      const stream = this.ports
        .streamsOf(node, port)
        .find((candidate) => candidate.kind === "audio" && candidate.source === source);
      const streamId = stream?.streamId ?? null;
      const refusal = streamId === null ? this.ports.refusalOf(node, port) : null;
      if (refusal !== entry.lastError && (refusal !== null || streamId !== null)) {
        entry.lastError = refusal;
        changed = true;
      }
      if (streamId === entry.streamId) {
        continue;
      }
      entry.streamId = streamId;
      if (streamId !== null) {
        entry.sink?.reset();
        entry.loss.reset();
      }
      changed = true;
    }
    if (changed) {
      this.notify();
    }
  };

  private readonly handleStatus = (connected: boolean): void => {
    if (connected) {
      for (const entry of this.entries.values()) {
        if (entry.desired && this.outputRunning) {
          this.ensureSink(entry);
        }
      }
    } else {
      this.dropLiveStreams();
    }
  };

  private readonly handleAudio = (frame: AudioFrame): void => {
    const entry = this.findByStream(frame.streamId);
    const sink = entry?.sink;
    if (!entry || !sink) {
      return;
    }
    const action = entry.loss.next(frame.timestamp);
    if (action.kind !== "continuous") {
      if (action.kind === "reset") {
        sink.reset();
      } else {
        sink.conceal(action.frames);
      }
      entry.lost += action.frames;
      this.notify();
    }
    const accepted = sink.push(
      frame.opus,
      Math.round(Number(frame.timestamp) * US_PER_FRAME),
      frame.chLayout,
    );
    if (!accepted) {
      const frames = entry.loss.packetFrames;
      if (frames !== null) {
        sink.conceal(frames);
        entry.lost += frames;
        this.notify();
      }
    }
  };

  private ensureEntry(tap: AudioTap): ChannelEntry {
    const key = tapKey(tap);
    let entry = this.entries.get(key);
    if (!entry) {
      entry = {
        tap: { node: tap.node, port: tap.port, source: tap.source },
        desired: false,
        hold: null,
        streamId: null,
        sink: null,
        sinkReady: false,
        sinkPending: false,
        generation: 0,
        volume: 1,
        lastError: null,
        loss: new LossTracker(MAX_GAP_FRAMES),
        lost: 0,
        underruns: 0,
        bufferedFrames: 0,
        trimmedFrames: 0,
        sinkUnderruns: 0,
        decoderDroppedFrames: 0,
      };
      this.entries.set(key, entry);
    }
    return entry;
  }

  private findByStream(streamId: number): ChannelEntry | undefined {
    for (const entry of this.entries.values()) {
      if (entry.streamId === streamId) {
        return entry;
      }
    }
    return undefined;
  }

  private ensureSink(entry: ChannelEntry): void {
    if (entry.sink) {
      this.sendSubscribe(entry);
      return;
    }
    if (entry.sinkPending) {
      return;
    }
    entry.sinkPending = true;
    const generation = entry.generation;
    this.createSink(
      keyOf(entry),
      entry.volume,
      (err) => {
        if (entry.desired && entry.generation === generation) this.fail(entry, err);
      },
      (report) => {
        if (entry.desired && entry.generation === generation) this.observe(entry, report);
      },
    )
      .then((sink) => {
        entry.sinkPending = false;
        if (!entry.desired) {
          sink.close();
          return;
        }
        if (generation !== entry.generation) {
          sink.close();
          this.ensureSink(entry);
          return;
        }
        entry.sink = sink;
        entry.sinkUnderruns = 0;
        entry.decoderDroppedFrames = 0;
        entry.bufferedFrames = 0;
        entry.trimmedFrames = 0;
        sink.setVolume(entry.volume);
        void Promise.resolve(sink.ready)
          .then(() => {
            if (entry.sink === sink) {
              entry.sinkReady = true;
              this.sendSubscribe(entry);
            }
          })
          .catch((err: unknown) => {
            if (entry.sink === sink) this.fail(entry, err);
          });
      })
      .catch((err: unknown) => {
        entry.sinkPending = false;
        if (entry.generation !== generation) {
          if (entry.desired) this.ensureSink(entry);
          return;
        }
        this.fail(entry, err);
      });
  }

  private sendSubscribe(entry: ChannelEntry): void {
    if (!entry.desired || entry.hold !== null || !entry.sinkReady || !this.outputRunning) {
      return;
    }
    entry.hold = this.ports.hold(entry.tap.node, entry.tap.port);
    this.followStreams();
  }

  getBufferedMs(tap: AudioTap): number {
    return ((this.entries.get(tapKey(tap))?.bufferedFrames ?? 0) * 1000) / SAMPLE_RATE;
  }

  getTrimmedMs(tap: AudioTap): number {
    return ((this.entries.get(tapKey(tap))?.trimmedFrames ?? 0) * 1000) / SAMPLE_RATE;
  }

  private observe(entry: ChannelEntry, report: WorkletReport): void {
    const decoderDropped = report.decoderDroppedFrames ?? entry.decoderDroppedFrames;
    entry.lost += Math.max(0, decoderDropped - entry.decoderDroppedFrames);
    entry.decoderDroppedFrames = decoderDropped;
    const since = report.underruns - entry.sinkUnderruns;
    entry.bufferedFrames = report.bufferedFrames ?? entry.bufferedFrames;
    entry.trimmedFrames = report.trimmedFrames ?? entry.trimmedFrames;
    entry.sinkUnderruns = report.underruns;
    entry.underruns += Math.max(0, since);
    this.notify();
  }

  private fail(entry: ChannelEntry, err: unknown): void {
    console.error(`audio ${keyOf(entry)} failed:`, err);
    entry.lastError = err instanceof Error ? err.message : String(err);
    this.stop(entry.tap);
    this.notify();
  }

  private teardown(entry: ChannelEntry): void {
    entry.sink?.close();
    entry.sink = null;
    entry.sinkReady = false;
    entry.bufferedFrames = 0;
  }

  private dropLiveStreams(): void {
    let changed = false;
    for (const entry of this.entries.values()) {
      if (entry.streamId !== null) {
        entry.streamId = null;
        changed = true;
      }
    }
    if (changed) {
      this.notify();
    }
  }

  private notify(): void {
    for (const listener of this.storeListeners) {
      listener();
    }
  }
}
