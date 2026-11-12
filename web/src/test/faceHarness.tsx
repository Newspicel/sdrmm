import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ReactFlowProvider } from "@xyflow/react";
import type { ComponentType } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { type Workspace, WorkspaceProvider } from "../canvas/context";
import type {
  ChannelDescriptor,
  ChannelInfo,
  DeviceSet,
  NodeProblem,
  PatchCatalog,
  PatchGraph,
  PatchNode,
  ResolvedPatch,
  TrunkSystemStatus,
} from "../lib/types";
import { SdrSocket } from "../lib/ws";
import { CATALOG } from "./catalog";
import { resolvedBy } from "./recorded";

export interface FaceHarness {
  graph: PatchGraph;
  devices?: ReadonlyMap<string, DeviceSet>;
  channels?: ReadonlyMap<string, ChannelInfo>;
  owners?: ReadonlyMap<string, string>;
  catalog?: PatchCatalog;
  channelTypes?: readonly ChannelDescriptor[];
  resolved?: ResolvedPatch | null;
  problems?: readonly NodeProblem[];
  selected?: string | null;
  trunks?: readonly TrunkSystemStatus[];
}

const NO_DEVICES: ReadonlyMap<string, DeviceSet> = new Map();

export function stubWorkspace(harness: FaceHarness): Workspace {
  const devices = harness.devices ?? NO_DEVICES;
  const resolved =
    harness.resolved === undefined ? resolvedBy(harness.graph, devices) : harness.resolved;
  return {
    socket: new SdrSocket(),
    graph: harness.graph,
    rack: {},
    settings: {},
    context: {
      catalog: harness.catalog ?? CATALOG,
      channelTypes: harness.channelTypes ?? [],
      resolved,
      bound: devices,
    },
    resolved,
    problems: harness.problems ?? [],
    deviceSets: [...devices.values()],
    trunks: harness.trunks ?? [],
    devices,
    channels: harness.channels ?? new Map(),
    owners: harness.owners ?? new Map(),
    savedChannels: new Map(),
    selected: harness.selected ?? null,
    select: () => {},
    expanded: null,
    expand: () => {},
    talking: false,
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
