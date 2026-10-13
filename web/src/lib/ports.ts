import type { Listener, Unsubscribe } from "./listeners";
import type { ClientCommand, ServerEvent, StreamKind, StreamParams } from "./types";

export interface PortSocket {
  send(command: ClientCommand): void;
  on<K extends "status" | "event">(kind: K, listener: Listener<K>): Unsubscribe;
}

export interface PortStream {
  streamId: number;
  node: string;
  port: string;
  source: string;
  kind: StreamKind;
}

interface Held {
  node: string;
  port: string;
  count: number;
  params: StreamParams | undefined;
}

export function portKey(node: string, port: string): string {
  return `${node}\u0000${port}`;
}

export function streamSource(source: string, via: readonly string[], index: number): string {
  const path = [source, ...via].join("/");
  return index === 0 ? path : `${path}#${index}`;
}

export function sourceKey(node: string, port: string, source: string): string {
  return `${portKey(node, port)}\u0000${source}`;
}

export function producerOf(key: string): string {
  const source = key.split("\u0000").at(-1) ?? key;
  return source.split("/")[0]?.split("#")[0] ?? source;
}

export class PortHub {
  private socket: PortSocket | null = null;
  private unsubscribes: Unsubscribe[] = [];
  private readonly held = new Map<string, Held>();
  private readonly streams = new Map<number, PortStream>();
  private readonly refusals = new Map<string, string>();
  private readonly ended = new Set<string>();
  private readonly listeners = new Set<() => void>();

  private readonly onEvent = (event: ServerEvent): void => {
    if (event.type === "PortStream") {
      const { stream_id, node, port, source, via, index, kind } = event.data;
      const stream = {
        streamId: stream_id,
        node,
        port,
        source: streamSource(source, via ?? [], index ?? 0),
        kind,
      };
      this.streams.set(stream_id, stream);
      this.ended.delete(sourceKey(node, port, stream.source));
      this.refusals.delete(portKey(node, port));
      this.notify();
    } else if (event.type === "PortRefused") {
      const { node, port, reason } = event.data;
      this.refusals.set(portKey(node, port), reason);
      this.notify();
    } else if (event.type === "StreamStopped") {
      const stream = this.streams.get(event.data.stream_id);
      if (stream?.kind !== event.data.kind) {
        return;
      }
      this.streams.delete(event.data.stream_id);
      this.ended.add(sourceKey(stream.node, stream.port, stream.source));
      this.notify();
    }
  };

  private readonly onStatus = (connected: boolean): void => {
    this.streams.clear();
    this.refusals.clear();
    this.ended.clear();
    this.notify();
    if (connected) {
      for (const held of this.held.values()) {
        this.send(held, true);
      }
    }
  };

  readonly subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  attach(socket: PortSocket): void {
    if (this.socket === socket) {
      return;
    }
    this.detach();
    this.socket = socket;
    this.unsubscribes = [socket.on("event", this.onEvent), socket.on("status", this.onStatus)];
    for (const held of this.held.values()) {
      this.send(held, true);
    }
  }

  detach(): void {
    this.socket = null;
    for (const unsubscribe of this.unsubscribes) {
      unsubscribe();
    }
    this.unsubscribes = [];
    this.streams.clear();
  }

  hold(node: string, port: string, params?: StreamParams): () => void {
    const key = portKey(node, port);
    const held = this.held.get(key) ?? { node, port, count: 0, params };
    held.count += 1;
    this.held.set(key, held);
    if (held.count === 1) {
      held.params = params;
      this.send(held, true);
    } else {
      this.tune(node, port, params);
    }
    let released = false;
    return () => {
      if (!released) {
        released = true;
        this.release(key);
      }
    };
  }

  tune(node: string, port: string, params: StreamParams | undefined): void {
    const held = this.held.get(portKey(node, port));
    if (held === undefined || params === undefined || sameParams(held.params, params)) {
      return;
    }
    held.params = params;
    this.send(held, true);
  }

  streamOf(streamId: number): PortStream | undefined {
    return this.streams.get(streamId);
  }

  streamsOf(node: string, port: string): PortStream[] {
    return [...this.streams.values()].filter(
      (stream) => stream.node === node && stream.port === port,
    );
  }

  hasEnded(node: string, port: string, source: string): boolean {
    return this.ended.has(sourceKey(node, port, source));
  }

  refusalOf(node: string, port: string): string | null {
    return this.refusals.get(portKey(node, port)) ?? null;
  }

  private release(key: string): void {
    const held = this.held.get(key);
    if (held === undefined) {
      return;
    }
    held.count -= 1;
    if (held.count > 0) {
      return;
    }
    this.held.delete(key);
    this.refusals.delete(key);
    for (const ended of this.ended) {
      if (ended.startsWith(`${key}\u0000`)) {
        this.ended.delete(ended);
      }
    }
    for (const [id, stream] of this.streams) {
      if (portKey(stream.node, stream.port) === key) {
        this.streams.delete(id);
      }
    }
    this.send(held, false);
    this.notify();
  }

  private send(held: Held, on: boolean): void {
    const { node, port } = held;
    this.socket?.send(
      on
        ? {
            type: "SubscribePort",
            data: held.params === undefined ? { node, port } : { node, port, params: held.params },
          }
        : { type: "UnsubscribePort", data: { node, port } },
    );
  }

  private notify(): void {
    for (const listener of this.listeners) {
      listener();
    }
  }
}

function sameParams(a: StreamParams | undefined, b: StreamParams): boolean {
  return a !== undefined && a.fps === b.fps && a.bins === b.bins;
}

export const portHub = new PortHub();
