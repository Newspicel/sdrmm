import { useEffect } from "react";
import { create } from "zustand";
import type { Listener, Unsubscribe } from "./listeners";
import type {
  ClientCommand,
  DecodedRecord,
  DecodedRecordOf,
  DecoderEventOf,
  DecoderKind,
  Entity,
  EntityKey,
  EventKind,
  PositionTrack,
  PropagationView,
  ServerEvent,
  SinkFeed,
  SinkUpdate,
  SurfaceRef,
} from "./types";

export const SINK_RECORDS = 500;

export interface SinkView {
  entities: readonly Entity[];
  records: readonly DecodedRecord[];
  tracks: readonly PositionTrack[];
  surfaces: readonly SurfaceRef[];
  propagation: PropagationView | null;
  lost: number;
}

export type EntityOf<K extends DecoderKind> = Omit<Entity, "event"> & {
  event: DecoderEventOf<K>;
};

export interface SinkGroup {
  source: string;
  kind: EventKind;
  entities: readonly Entity[];
  records: readonly DecodedRecord[];
}

export const EMPTY_SINK: SinkView = Object.freeze({
  entities: [],
  records: [],
  tracks: [],
  surfaces: [],
  propagation: null,
  lost: 0,
});

export function entityKey(key: EntityKey): string {
  return `${key.source}\u0000${key.kind}\u0000${key.id}`;
}

export function snapshotView(feed: SinkFeed): SinkView {
  return {
    entities: feed.entities,
    records: feed.records.toReversed().slice(0, SINK_RECORDS),
    tracks: feed.tracks,
    surfaces: feed.surfaces,
    propagation: feed.propagation ?? null,
    lost: feed.lost,
  };
}

function mergedEntities(
  current: readonly Entity[],
  upserts: readonly Entity[],
  removed: readonly EntityKey[],
): readonly Entity[] {
  if (upserts.length === 0 && removed.length === 0) {
    return current;
  }
  const byKey = new Map(current.map((entity) => [entityKey(entity.key), entity]));
  for (const key of removed) {
    byKey.delete(entityKey(key));
  }
  for (const entity of upserts) {
    byKey.set(entityKey(entity.key), entity);
  }
  return [...byKey.values()];
}

export function updatedView(view: SinkView, update: SinkUpdate): SinkView {
  const added = update.records ?? [];
  return {
    entities: mergedEntities(view.entities, update.upserts ?? [], update.removed ?? []),
    records:
      added.length === 0
        ? view.records
        : added.toReversed().concat(view.records).slice(0, SINK_RECORDS),
    tracks: (update.tracks?.length ?? 0) > 0 ? (update.tracks ?? []) : view.tracks,
    surfaces: update.surfaces ?? view.surfaces,
    propagation: update.propagation ?? view.propagation,
    lost: update.lost,
  };
}

interface SinkStore {
  sinks: Readonly<Record<string, SinkView>>;
  observe: (event: ServerEvent) => void;
  forget: (node: string) => void;
  reset: () => void;
}

export const useSinkStore = create<SinkStore>((set) => ({
  sinks: {},
  observe: (event) => {
    if (event.type === "SinkSnapshot") {
      const { node, feed } = event.data;
      set((state) => ({ sinks: { ...state.sinks, [node]: snapshotView(feed) } }));
    } else if (event.type === "SinkUpdate") {
      const { node, update } = event.data;
      set((state) => ({
        sinks: { ...state.sinks, [node]: updatedView(state.sinks[node] ?? EMPTY_SINK, update) },
      }));
    }
  },
  forget: (node) =>
    set((state) => {
      const { [node]: _gone, ...rest } = state.sinks;
      return { sinks: rest };
    }),
  reset: () => set({ sinks: {} }),
}));

export interface SinkSocket {
  send(command: ClientCommand): void;
  isConnected(): boolean;
  on<K extends "status" | "event">(kind: K, listener: Listener<K>): Unsubscribe;
}

export class SinkHub {
  private socket: SinkSocket | null = null;
  private unsubscribes: Unsubscribe[] = [];
  private readonly watchers = new Map<string, number>();

  attach(socket: SinkSocket): void {
    if (this.socket === socket) {
      return;
    }
    this.detach();
    this.socket = socket;
    this.unsubscribes = [
      socket.on("event", useSinkStore.getState().observe),
      socket.on("status", (connected) => {
        if (connected) {
          this.resubscribe();
        }
      }),
    ];
    this.resubscribe();
  }

  detach(): void {
    this.socket = null;
    for (const unsubscribe of this.unsubscribes) {
      unsubscribe();
    }
    this.unsubscribes = [];
  }

  watch(node: string): () => void {
    const count = this.watchers.get(node) ?? 0;
    this.watchers.set(node, count + 1);
    if (count === 0) {
      this.send({ type: "SubscribeSink", data: { node } });
    }
    return () => {
      const left = (this.watchers.get(node) ?? 1) - 1;
      if (left > 0) {
        this.watchers.set(node, left);
        return;
      }
      this.watchers.delete(node);
      this.send({ type: "UnsubscribeSink", data: { node } });
      useSinkStore.getState().forget(node);
    };
  }

  watched(): string[] {
    return [...this.watchers.keys()];
  }

  private resubscribe(): void {
    for (const node of this.watchers.keys()) {
      this.send({ type: "SubscribeSink", data: { node } });
    }
  }

  private send(command: ClientCommand): void {
    if (this.socket?.isConnected() === true) {
      this.socket.send(command);
    }
  }
}

export const sinkHub = new SinkHub();

export function useSinkFeed(node: string): SinkView {
  useEffect(() => sinkHub.watch(node), [node]);
  return useSinkStore((state) => state.sinks[node] ?? EMPTY_SINK);
}

function sourceOf(record: DecodedRecord): string {
  return record.origin?.node ?? "";
}

export function groupsOf(view: SinkView): SinkGroup[] {
  const groups = new Map<string, SinkGroup>();
  const groupOf = (source: string, kind: EventKind): SinkGroup => {
    const key = `${source}\u0000${kind}`;
    let group = groups.get(key);
    if (group === undefined) {
      group = { source, kind, entities: [], records: [] };
      groups.set(key, group);
    }
    return group;
  };
  for (const record of view.records) {
    (groupOf(sourceOf(record), record.event.kind).records as DecodedRecord[]).push(record);
  }
  for (const entity of view.entities) {
    (groupOf(entity.key.source, entity.key.kind).entities as Entity[]).push(entity);
  }
  return [...groups.values()].toSorted(
    (a, b) => a.source.localeCompare(b.source) || a.kind.localeCompare(b.kind),
  );
}

export function recordsOf<K extends DecoderKind>(
  group: SinkGroup,
  kind: K,
): readonly DecodedRecordOf<K>[] {
  return group.kind === kind ? (group.records as readonly DecodedRecordOf<K>[]) : [];
}

export function entitiesOf<K extends DecoderKind>(
  group: SinkGroup,
  kind: K,
): readonly EntityOf<K>[] {
  return group.kind === kind ? (group.entities as readonly EntityOf<K>[]) : [];
}

export function lastSeenMs(entity: Pick<Entity, "last_seen">): number {
  const at = Date.parse(entity.last_seen);
  return Number.isNaN(at) ? Date.now() : at;
}
