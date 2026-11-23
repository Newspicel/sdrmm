import {
  VIDEO_PACING_LEAD_MS,
  VIDEO_PRESENTATION_MASK,
  VIDEO_PRESENTATION_TIMESTAMP,
  type VideoFrame,
} from "./frame";
import type { Listener, Unsubscribe } from "./listeners";
import { mediaLatencyMs } from "./mediaLatency";
import { type PortHub, portHub, producerOf, sourceKey } from "./ports";

export interface VideoSocket {
  on<K extends "video" | "status">(kind: K, listener: Listener<K>): Unsubscribe;
}

const RELEASE_GRACE_MS = 5_000;

const PENDING_FRAMES = 32;
const PENDING_BYTES = 16 * 1024 * 1024;
const PTS_PERIOD = VIDEO_PRESENTATION_MASK + 1n;
const AUDIO_SLEW_MS = 2;

export interface VideoPresentation {
  at: number;
  epoch: number;
}

class PictureClock {
  private anchor: { pts: bigint; at: number; stream: number; audioLatency: number } | null = null;
  private epoch = 0;

  at(
    timestamp: bigint,
    stream: number,
    now: number,
    audioLatency: number,
  ): VideoPresentation | undefined {
    if ((timestamp & VIDEO_PRESENTATION_TIMESTAMP) === 0n) return undefined;
    const pts = timestamp & VIDEO_PRESENTATION_MASK;
    const anchor = this.anchor;
    if (anchor?.stream === stream) {
      const distance =
        ((pts - anchor.pts + PTS_PERIOD / 2n) & VIDEO_PRESENTATION_MASK) - PTS_PERIOD / 2n;
      const at = anchor.at + Number(distance) / 90;
      const lead = at - now;
      if (lead >= -120 && lead <= 500) {
        anchor.audioLatency += Math.max(
          -AUDIO_SLEW_MS,
          Math.min(AUDIO_SLEW_MS, audioLatency - anchor.audioLatency),
        );
        return { at: at + anchor.audioLatency, epoch: this.epoch };
      }
    }
    const at = now + VIDEO_PACING_LEAD_MS;
    this.anchor = { pts, stream, at, audioLatency };
    this.epoch += 1;
    return { at: at + audioLatency, epoch: this.epoch };
  }

  reset(): void {
    this.anchor = null;
  }
}

export interface VideoInput {
  node: string;
  port: string;
  source: string;
}

type PictureListener = (frame: VideoFrame, presentation?: VideoPresentation) => void;

interface Watched {
  input: VideoInput;
  listeners: Set<PictureListener>;
  latest: VideoFrame | null;
  release: number;
  hold: (() => void) | null;
  pending: { frame: VideoFrame; due: number }[];
  pendingBytes: number;
  dropped: number;
  missing: number;
  lastSequence: { stream: number; seq: number } | null;
  timer: number;
  clock: PictureClock;
}

function keyOf(input: VideoInput): string {
  return sourceKey(input.node, input.port, input.source);
}

export class VideoHub {
  private socket: VideoSocket | null = null;
  private unsubscribes: Unsubscribe[] = [];
  private readonly inputs = new Map<string, Watched>();
  private readonly dropWatchers = new Set<() => void>();
  private hidden = false;

  constructor(private readonly ports: PortHub = portHub) {}

  private readonly onFrame = (frame: VideoFrame): void => {
    const stream = this.ports.streamOf(frame.streamId);
    if (stream === undefined || stream.kind !== "video") {
      return;
    }
    const key = sourceKey(stream.node, stream.port, stream.source);
    const watched = this.inputs.get(key);
    if (watched === undefined) {
      return;
    }
    const last = watched.lastSequence;
    if (last?.stream === frame.streamId) {
      const step = (frame.seq - last.seq) >>> 0;
      if (step > 1 && step < 0x80000000) watched.missing += step - 1;
    }
    watched.lastSequence = { stream: frame.streamId, seq: frame.seq };
    const audioLatency = mediaLatencyMs(producerOf(key));
    const presentation = watched.clock.at(
      frame.timestamp,
      frame.streamId,
      performance.now(),
      audioLatency,
    );
    if (presentation !== undefined) {
      watched.latest = frame;
      for (const listener of watched.listeners) listener(frame, presentation);
      return;
    }
    const due = Date.now() + audioLatency;
    watched.pending.push({ frame, due });
    watched.pendingBytes += frame.jpeg.byteLength;
    let dropped = 0;
    while (watched.pending.length > PENDING_FRAMES || watched.pendingBytes > PENDING_BYTES) {
      const oldest = watched.pending.shift();
      if (oldest === undefined) {
        break;
      }
      watched.pendingBytes -= oldest.frame.jpeg.byteLength;
      dropped += 1;
    }
    if (dropped > 0) {
      watched.dropped += dropped;
      for (const watcher of this.dropWatchers) {
        watcher();
      }
    }
    this.present(watched);
  };

  readonly onDrop = (listener: () => void): (() => void) => {
    this.dropWatchers.add(listener);
    return () => {
      this.dropWatchers.delete(listener);
    };
  };

  droppedFrames(input: VideoInput): number {
    return this.inputs.get(keyOf(input))?.dropped ?? 0;
  }

  missingFrames(input: VideoInput): number {
    return this.inputs.get(keyOf(input))?.missing ?? 0;
  }

  private present(watched: Watched): void {
    clearTimeout(watched.timer);
    while (watched.pending[0] && watched.pending[0].due <= Date.now()) {
      const next = watched.pending.shift();
      if (!next) break;
      watched.pendingBytes -= next.frame.jpeg.byteLength;
      if (this.ports.streamOf(next.frame.streamId) === undefined) continue;
      watched.latest = next.frame;
      for (const listener of watched.listeners) listener(next.frame);
    }
    const next = watched.pending[0];
    watched.timer = next
      ? setTimeout(() => this.present(watched), Math.max(0, next.due - Date.now()))
      : 0;
  }

  private clear(watched: Watched): void {
    clearTimeout(watched.timer);
    watched.pending = [];
    watched.pendingBytes = 0;
    watched.lastSequence = null;
    watched.clock.reset();
    watched.timer = 0;
  }

  private readonly onStatus = (): void => {
    for (const watched of this.inputs.values()) this.clear(watched);
  };

  attach(socket: VideoSocket): void {
    if (this.socket === socket) {
      return;
    }
    this.detach();
    this.socket = socket;
    this.unsubscribes = [socket.on("video", this.onFrame), socket.on("status", this.onStatus)];
  }

  detach(): void {
    this.socket = null;
    for (const watched of this.inputs.values()) this.clear(watched);
    for (const unsubscribe of this.unsubscribes) {
      unsubscribe();
    }
    this.unsubscribes = [];
  }

  subscribe(input: VideoInput, listener: PictureListener): () => void {
    const key = keyOf(input);
    let watched = this.inputs.get(key);
    if (watched === undefined) {
      watched = {
        input,
        listeners: new Set(),
        latest: null,
        release: 0,
        hold: null,
        pending: [],
        pendingBytes: 0,
        dropped: 0,
        missing: 0,
        lastSequence: null,
        timer: 0,
        clock: new PictureClock(),
      };
      this.inputs.set(key, watched);
      this.hold(watched);
    } else if (watched.release !== 0) {
      clearTimeout(watched.release);
      watched.release = 0;
    }
    watched.listeners.add(listener);
    return () => this.release(key, listener);
  }

  setVisible(visible: boolean): void {
    if (this.hidden === !visible) {
      return;
    }
    this.hidden = !visible;
    for (const watched of this.inputs.values()) {
      this.clear(watched);
      if (visible) {
        this.hold(watched);
      } else {
        watched.hold?.();
        watched.hold = null;
      }
    }
  }

  latest(input: VideoInput): VideoFrame | null {
    return this.inputs.get(keyOf(input))?.latest ?? null;
  }

  private hold(watched: Watched): void {
    if (this.hidden || watched.hold !== null) {
      return;
    }
    watched.hold = this.ports.hold(watched.input.node, watched.input.port);
  }

  private release(key: string, listener: PictureListener): void {
    const watched = this.inputs.get(key);
    if (watched === undefined) {
      return;
    }
    watched.listeners.delete(listener);
    if (watched.listeners.size > 0 || watched.release !== 0) {
      return;
    }
    watched.release = setTimeout(() => {
      this.clear(watched);
      this.inputs.delete(key);
      watched.hold?.();
      watched.hold = null;
    }, RELEASE_GRACE_MS);
  }
}

export const videoHub = new VideoHub();
