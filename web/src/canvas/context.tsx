import {
  createContext,
  type ReactNode,
  useContext,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import type {
  ChannelInfo,
  ChannelSettings,
  DeviceSet,
  NodeProblem,
  NodeStream,
  PatchGraph,
  RackLayout,
  ResolvedPatch,
  TrunkSystemStatus,
  WorkspaceSettings,
  WorkspaceSnapshot,
} from "../lib/types";
import type { SdrSocket } from "../lib/ws";
import type { GraphContext } from "./graph";

export interface Workspace {
  id: number;
  socket: SdrSocket;
  graph: PatchGraph;
  rack: RackLayout;
  settings: WorkspaceSettings;
  context: GraphContext;
  resolved: ResolvedPatch | null;
  problems: readonly NodeProblem[];
  deviceSets: readonly DeviceSet[];
  trunks: readonly TrunkSystemStatus[];
  /** Streams nodes make themselves, a voice or an encoder's baseband, with their sinks. */
  nodeStreams: readonly NodeStream[];
  devices: ReadonlyMap<string, DeviceSet>;
  channels: ReadonlyMap<string, ChannelInfo>;
  owners: ReadonlyMap<string, string>;
  savedChannels: ReadonlyMap<string, ChannelSettings>;
  selected: string | null;
  select: (node: string | null) => void;
  expanded: string | null;
  expand: (node: string | null) => void;
  inspected: string | null;
  inspect: (node: string | null) => void;
  /** The push-to-talk key is held. */
  talking: boolean;
  edit: (edit: (snapshot: WorkspaceSnapshot) => WorkspaceSnapshot) => void;
  editSettings: (settings: Partial<WorkspaceSettings>) => void;
  apply: () => void;
}

/** Holds the newest workspace and tells readers when it changes, so a reader re-renders only
 * when what it selects changes, not on every change to the workspace. */
class WorkspaceHub {
  private readonly listeners = new Set<() => void>();

  constructor(private held: Workspace) {}

  get current(): Workspace {
    return this.held;
  }

  readonly subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  publish(next: Workspace): void {
    if (next === this.held) {
      return;
    }
    this.held = next;
    for (const listener of this.listeners) {
      listener();
    }
  }
}

const WorkspaceContext = createContext<WorkspaceHub | null>(null);

export function WorkspaceProvider({ value, children }: { value: Workspace; children: ReactNode }) {
  const [hub] = useState(() => new WorkspaceHub(value));
  useLayoutEffect(() => hub.publish(value), [hub, value]);
  return <WorkspaceContext value={hub}>{children}</WorkspaceContext>;
}

function useHub(): WorkspaceHub {
  const hub = useContext(WorkspaceContext);
  if (hub === null) {
    throw new Error("useWorkspaceContext outside WorkspaceProvider");
  }
  return hub;
}

/** What `select` picks from the workspace. The reader re-renders only when the pick changes by
 * value, so a node face selects its own slice and stays still while others change. */
export function useWorkspaceSelect<T>(
  select: (workspace: Workspace) => T,
  same: (a: T, b: T) => boolean = samePick,
): T {
  const hub = useHub();
  const kept = useRef<{ value: T } | null>(null);
  const snapshot = (): T => {
    const picked = select(hub.current);
    const held = kept.current;
    if (held !== null && same(held.value, picked)) {
      return held.value;
    }
    kept.current = { value: picked };
    return picked;
  };
  return useSyncExternalStore(hub.subscribe, snapshot, snapshot);
}

/** Reads the workspace when called, for handlers that act on it without re-rendering on it. */
export function useWorkspaceNow(): () => Workspace {
  const hub = useHub();
  return () => hub.current;
}

/** The whole workspace: re-renders on every change. For one-of-a-kind views, never a node face. */
export function useWorkspaceContext(): Workspace {
  return useWorkspaceSelect(whole, Object.is);
}

function whole(workspace: Workspace): Workspace {
  return workspace;
}

/** Plain data compares by value; anything else, a Map or a socket, by identity. */
export function samePick(a: unknown, b: unknown): boolean {
  if (Object.is(a, b)) {
    return true;
  }
  if (Array.isArray(a)) {
    return Array.isArray(b) && a.length === b.length && a.every((item, i) => samePick(item, b[i]));
  }
  if (!plain(a) || !plain(b)) {
    return false;
  }
  const keys = Object.keys(a);
  return (
    keys.length === Object.keys(b).length &&
    keys.every((key) => Object.hasOwn(b, key) && samePick(a[key], b[key]))
  );
}

function plain(value: unknown): value is Record<string, unknown> {
  return (
    typeof value === "object" && value !== null && Object.getPrototypeOf(value) === Object.prototype
  );
}
