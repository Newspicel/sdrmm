import type { Listener, SocketEvents, Unsubscribe } from "./listeners";
import { type PortHub, portHub, portKey } from "./ports";
import type { StreamKind } from "./types";

export type TapKind = Extract<StreamKind, "iq" | "symbols">;

export interface TapSocket<K extends TapKind> {
  on(kind: K, listener: Listener<K>): Unsubscribe;
}

export const RELEASE_GRACE_MS = 5_000;

interface Watched<F> {
  node: string;
  port: string;
  listeners: Set<(frame: F) => void>;
  latest: F | null;
  release: number;
  hold: (() => void) | null;
}

export class PortTaps<K extends TapKind> {
  private socket: TapSocket<K> | null = null;
  private unsubscribe: Unsubscribe | null = null;
  private readonly taps = new Map<string, Watched<SocketEvents[K]>>();
  private hidden = false;

  constructor(
    private readonly kind: K,
    private readonly ports: PortHub = portHub,
  ) {}

  private readonly onFrame = (frame: SocketEvents[K]): void => {
    const stream = this.ports.streamOf(frame.streamId);
    if (stream === undefined || stream.kind !== this.kind) {
      return;
    }
    const tap = this.taps.get(portKey(stream.node, stream.port));
    if (tap === undefined) {
      return;
    }
    tap.latest = frame;
    for (const listener of tap.listeners) {
      listener(frame);
    }
  };

  attach(socket: TapSocket<K>): void {
    if (this.socket === socket) {
      return;
    }
    this.detach();
    this.socket = socket;
    this.unsubscribe = socket.on(this.kind, this.onFrame);
  }

  detach(): void {
    this.socket = null;
    this.unsubscribe?.();
    this.unsubscribe = null;
  }

  subscribe(node: string, port: string, listener: (frame: SocketEvents[K]) => void): () => void {
    const key = portKey(node, port);
    let tap = this.taps.get(key);
    if (tap === undefined) {
      tap = { node, port, listeners: new Set(), latest: null, release: 0, hold: null };
      this.taps.set(key, tap);
      this.hold(tap);
    } else if (tap.release !== 0) {
      clearTimeout(tap.release);
      tap.release = 0;
    }
    tap.listeners.add(listener);
    return () => this.release(key, listener);
  }

  setVisible(visible: boolean): void {
    if (this.hidden === !visible) {
      return;
    }
    this.hidden = !visible;
    for (const tap of this.taps.values()) {
      if (visible) {
        this.hold(tap);
      } else {
        tap.hold?.();
        tap.hold = null;
      }
    }
  }

  latest(node: string, port: string): SocketEvents[K] | null {
    return this.taps.get(portKey(node, port))?.latest ?? null;
  }

  private hold(tap: Watched<SocketEvents[K]>): void {
    if (this.hidden || tap.hold !== null) {
      return;
    }
    tap.hold = this.ports.hold(tap.node, tap.port);
  }

  private release(key: string, listener: (frame: SocketEvents[K]) => void): void {
    const tap = this.taps.get(key);
    if (tap === undefined) {
      return;
    }
    tap.listeners.delete(listener);
    if (tap.listeners.size > 0 || tap.release !== 0) {
      return;
    }
    tap.release = setTimeout(() => {
      this.taps.delete(key);
      tap.hold?.();
      tap.hold = null;
    }, RELEASE_GRACE_MS);
  }
}

export const iqHub = new PortTaps("iq");
export const symbolHub = new PortTaps("symbols");
