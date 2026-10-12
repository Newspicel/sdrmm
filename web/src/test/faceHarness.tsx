import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ReactFlowProvider } from "@xyflow/react";
import type { ComponentType } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { type Workspace, WorkspaceProvider } from "../canvas/context";
import type {
  ChannelInfo,
  DeviceSet,
  NodeProblem,
  PatchCatalog,
  PatchGraph,
  PatchNode,
  ResolvedPatch,
} from "../lib/types";
import { SdrSocket } from "../lib/ws";
import { CATALOG } from "./catalog";
import { wiredPatch } from "./resolved";

export interface FaceHarness {
  graph: PatchGraph;
  devices?: ReadonlyMap<string, DeviceSet>;
  channels?: ReadonlyMap<string, ChannelInfo>;
  owners?: ReadonlyMap<string, string>;
  catalog?: PatchCatalog;
  resolved?: ResolvedPatch | null;
  problems?: readonly NodeProblem[];
  selected?: string | null;
}

const NO_DEVICES: ReadonlyMap<string, DeviceSet> = new Map();

export function stubWorkspace(harness: FaceHarness): Workspace {
  const devices = harness.devices ?? NO_DEVICES;
  const resolved =
    harness.resolved === undefined
      ? wiredPatch(harness.graph, {
          devices,
          channels: harness.channels,
          owners: harness.owners,
        })
      : harness.resolved;
  return {
    socket: new SdrSocket(),
    graph: harness.graph,
    rack: {},
    settings: {},
    context: {
      catalog: harness.catalog ?? CATALOG,
      channelTypes: [],
      resolved,
      bound: devices,
    },
    resolved,
    problems: harness.problems ?? [],
    deviceSets: [...devices.values()],
    trunks: [],
    devices,
    channels: harness.channels ?? new Map(),
    owners: harness.owners ?? new Map(),
    savedChannels: new Map(),
    saveChannel: () => {},
    selected: harness.selected ?? null,
    select: () => {},
    expanded: null,
    expand: () => {},
    edit: () => {},
    editSettings: () => {},
    apply: () => {},
  };
}

export function renderFace(
  Face: ComponentType<{ node: PatchNode }>,
  node: PatchNode,
  harness: FaceHarness,
): string {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return renderToStaticMarkup(
    <QueryClientProvider client={queryClient}>
      <ReactFlowProvider>
        <WorkspaceProvider value={stubWorkspace(harness)}>
          <Face node={node} />
        </WorkspaceProvider>
      </ReactFlowProvider>
    </QueryClientProvider>,
  );
}
