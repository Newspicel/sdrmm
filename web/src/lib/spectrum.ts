import type { SpectrumFrame } from "./frame";
import { AUDIO_RATE_HZ } from "./limits";
import type { Listener, Unsubscribe } from "./listeners";
import { type PortHub, portHub, portKey, sourceKey } from "./ports";

export interface SpectrumSocket {
  on<K extends "spectrum">(kind: K, listener: Listener<K>): Unsubscribe;
}

export const SPECTRUM_FPS = 30;
export const SPECTRUM_HIDDEN_FPS = 1;
export const SPECTRUM_BINS = 1024;
export const SPECTRUM_MAX_BINS = 4096;

export const SPECTRUM_HISTORY_ROWS = 1024;

export const AUDIO_SPECTRUM_BINS = 256;
export const AUDIO_SPECTRUM_FPS = 15;

export function audioNyquistHz(sampleRate = AUDIO_RATE_HZ): number {
  return sampleRate / 2;
}

const RELEASE_GRACE_MS = 5_000;
const MIN_LANE_BINS = 16;

export function resampleRows(
  rows: Uint8Array,
  count: number,
  fromBins: number,
  toBins: number,
): Uint8Array<ArrayBuffer> {
  const out = new Uint8Array(count * toBins);
  if (fromBins === 0 || toBins === 0) {
    return out;
  }
  for (let row = 0; row < count; row++) {
    const from = row * fromBins;
    const to = row * toBins;
    for (let x = 0; x < toBins; x++) {
      const lo = Math.min(fromBins - 1, Math.floor((x * fromBins) / toBins));
      const hi = Math.min(fromBins - 1, Math.max(lo, Math.ceil(((x + 1) * fromBins) / toBins) - 1));
      let peak = 0;
      for (let i = lo; i <= hi; i++) {
        const value = rows[from + i] ?? 0;
        if (value > peak) {
          peak = value;
        }
      }
      out[to + x] = peak;
    }
  }
  return out;
}

interface Want {
  bins: number;
  fps: number;
}

export interface SpectrumWant {
  bins?: number;
  fps?: number;
  source?: string;
}

function resolveBins(desired: Iterable<Want>): number {
  let top = 0;
  for (const want of desired) {
    top = Math.max(top, want.bins);
  }
  return Math.min(SPECTRUM_MAX_BINS, Math.max(MIN_LANE_BINS, top));
}

function resolveFps(desired: Iterable<Want>): number {
  let top = 0;
  for (const want of desired) {
    top = Math.max(top, want.fps);
  }
  return Math.max(1, top);
}

export interface Lane {
  node: string;
  port: string;
  source?: string;
}

function laneKey(node: string, port: string, source?: string): string {
  return source === undefined ? portKey(node, port) : sourceKey(node, port, source);
}

export interface RowMeta {
  centerHz: number;
  spanHz: number;
  dbMin: number;
  dbMax: number;
  floorDb: number;
  at: number;
}

export interface SpectrumHistory {
  rows: Uint8Array;
  count: number;
  bins: number;
  meta: RowMeta[];
}

class History {
  private ring = new Uint8Array(0);
  private meta: (RowMeta | undefined)[] = [];
  private bins = 0;
  private write = 0;
  private filled = 0;
  latest: SpectrumFrame | null = null;

  record(frame: SpectrumFrame): void {
    this.latest = frame;
    if (frame.bins.length === 0) {
      return;
    }
    if (frame.bins.length !== this.bins) {
      if (this.filled > 0) {
        this.ring = resampleRows(this.ring, SPECTRUM_HISTORY_ROWS, this.bins, frame.bins.length);
      } else {
        this.ring = new Uint8Array(frame.bins.length * SPECTRUM_HISTORY_ROWS);
        this.meta = [];
        this.write = 0;
        this.filled = 0;
      }
      this.bins = frame.bins.length;
    }
    this.ring.set(frame.bins, this.write * this.bins);
    this.meta[this.write] = {
      centerHz: frame.centerHz,
      spanHz: frame.spanHz,
      dbMin: frame.dbMin,
      dbMax: frame.dbMax,
      floorDb: frame.floorDb,
      at: Date.now(),
    };
    this.write = (this.write + 1) % SPECTRUM_HISTORY_ROWS;
    this.filled = Math.min(this.filled + 1, SPECTRUM_HISTORY_ROWS);
  }

  read(): SpectrumHistory {
    const rows = new Uint8Array(this.filled * this.bins);
    const first = (this.write - this.filled + SPECTRUM_HISTORY_ROWS) % SPECTRUM_HISTORY_ROWS;
    const head = Math.min(this.filled, SPECTRUM_HISTORY_ROWS - first);
    rows.set(this.ring.subarray(first * this.bins, (first + head) * this.bins));
    rows.set(this.ring.subarray(0, (this.filled - head) * this.bins), head * this.bins);
    const meta: RowMeta[] = [];
    for (let i = 0; i < this.filled; i++) {
      const row = this.meta[(first + i) % SPECTRUM_HISTORY_ROWS];
      if (row !== undefined) {
        meta.push(row);
      }
    }
    return { rows, count: this.filled, bins: this.bins, meta };
  }
}

interface Watched {
  lane: Lane;
  listeners: Map<(frame: SpectrumFrame) => void, Want>;
  history: History;
  release: number;
  bins: number;
  fps: number;
  hold: (() => void) | null;
}

export class SpectrumHub {
  private socket: SpectrumSocket | null = null;
  private unsubscribes: Unsubscribe[] = [];
  private readonly lanes = new Map<string, Watched>();
  private hidden = false;

  constructor(private readonly ports: PortHub = portHub) {}

  private readonly onFrame = (frame: SpectrumFrame): void => {
    const stream = this.ports.streamOf(frame.streamId);
    if (stream === undefined || stream.kind !== "spectrum") {
      return;
    }
    const lane =
      this.lanes.get(sourceKey(stream.node, stream.port, stream.source)) ??
      this.lanes.get(portKey(stream.node, stream.port));
    if (lane === undefined) {
      return;
    }
    lane.history.record(frame);
    for (const listener of lane.listeners.keys()) {
      listener(frame);
    }
  };

  attach(socket: SpectrumSocket): void {
    if (this.socket === socket) {
      return;
    }
    this.detach();
    this.socket = socket;
    this.unsubscribes = [socket.on("spectrum", this.onFrame)];
  }

  detach(): void {
    this.socket = null;
    for (const unsubscribe of this.unsubscribes) {
      unsubscribe();
    }
    this.unsubscribes = [];
  }

  subscribe(
    node: string,
    port: string,
    listener: (frame: SpectrumFrame) => void,
    options: SpectrumWant = {},
  ): () => void {
    const { bins = SPECTRUM_BINS, fps = SPECTRUM_FPS, source } = options;
    const want: Want = { bins, fps };
    const key = laneKey(node, port, source);
    let lane = this.lanes.get(key);
    if (lane === undefined) {
      lane = {
        lane: { node, port, ...(source === undefined ? {} : { source }) },
        listeners: new Map([[listener, want]]),
        history: new History(),
        release: 0,
        bins: resolveBins([want]),
        fps: resolveFps([want]),
        hold: null,
      };
      this.lanes.set(key, lane);
      this.send(lane);
      return () => this.release(key, listener);
    }
    if (lane.release !== 0) {
      clearTimeout(lane.release);
      lane.release = 0;
    }
    lane.listeners.set(listener, want);
    this.refresh(lane);
    return () => this.release(key, listener);
  }

  setVisible(visible: boolean): void {
    if (this.hidden === !visible) {
      return;
    }
    this.hidden = !visible;
    for (const lane of this.lanes.values()) {
      this.send(lane);
    }
  }

  history(node: string, port: string, source?: string): SpectrumHistory {
    return (
      this.lanes.get(laneKey(node, port, source))?.history.read() ?? {
        rows: new Uint8Array(0),
        count: 0,
        bins: 0,
        meta: [],
      }
    );
  }

  latest(node: string, port: string, source?: string): SpectrumFrame | null {
    return this.lanes.get(laneKey(node, port, source))?.history.latest ?? null;
  }

  watched(): Lane[] {
    return [...this.lanes.values()].map((watched) => watched.lane);
  }

  private release(key: string, listener: (frame: SpectrumFrame) => void): void {
    const watched = this.lanes.get(key);
    if (watched === undefined) {
      return;
    }
    watched.listeners.delete(listener);
    if (watched.listeners.size > 0) {
      this.refresh(watched);
      return;
    }
    if (watched.release !== 0) {
      return;
    }
    watched.release = setTimeout(() => {
      this.lanes.delete(key);
      watched.hold?.();
      watched.hold = null;
    }, RELEASE_GRACE_MS);
  }

  private refresh(watched: Watched): void {
    const bins = resolveBins(watched.listeners.values());
    const fps = resolveFps(watched.listeners.values());
    if (bins !== watched.bins || fps !== watched.fps) {
      watched.bins = bins;
      watched.fps = fps;
      this.send(watched);
    }
  }

  private send(watched: Watched): void {
    const params = {
      fps: this.hidden ? SPECTRUM_HIDDEN_FPS : watched.fps,
      bins: watched.bins,
      spectrum: true,
    };
    const { node, port } = watched.lane;
    if (watched.hold === null) {
      watched.hold = this.ports.hold(node, port, params);
    } else {
      this.ports.tune(node, port, params);
    }
  }
}

export const spectrumHub = new SpectrumHub();
