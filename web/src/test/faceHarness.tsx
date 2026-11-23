import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ReactFlowProvider } from "@xyflow/react";
import type { ComponentType } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { type Workspace, WorkspaceProvider } from "../canvas/context";
import { InlineInspector } from "../canvas/inspector";
import type {
  ChannelDescriptor,
  ChannelInfo,
  DeviceSet,
  EncoderDescriptor,
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
  encoderTypes?: readonly EncoderDescriptor[];
  resolved?: ResolvedPatch | null;
  problems?: readonly NodeProblem[];
  selected?: string | null;
  trunks?: readonly TrunkSystemStatus[];
  /** Draws the face as inspected, its inspector content in place. */
  inspected?: boolean;
}

const NO_DEVICES: ReadonlyMap<string, DeviceSet> = new Map();

export function stubWorkspace(harness: FaceHarness): Workspace {
  const devices = harness.devices ?? NO_DEVICES;
  const resolved =
    harness.resolved === undefined ? resolvedBy(harness.graph, devices) : harness.resolved;
  return {
    id: 1,
    socket: new SdrSocket(),
    graph: harness.graph,
    rack: {},
    settings: {},
    context: {
      catalog: harness.catalog ?? CATALOG,
      channelTypes: harness.channelTypes ?? [],
      encoderTypes: harness.encoderTypes ?? [],
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
    inspected: null,
    inspect: () => {},
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
        <WorkspaceProvider
          value={{
            ...stubWorkspace(harness),
            inspected: harness.inspected === true ? node.id : null,
          }}
        >
          <InlineInspector>
            <Face node={node} />
          </InlineInspector>
        </WorkspaceProvider>
      </ReactFlowProvider>
    </QueryClientProvider>,
  );
}
