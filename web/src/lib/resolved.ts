import type { QueryClient } from "@tanstack/react-query";
import { useWorkspaceContext } from "../canvas/context";
import { RESOLVED_KEY } from "./api";
import { streamSource } from "./ports";
import type {
  ChannelAddr,
  ChannelInfo,
  DeviceSet,
  EventKind,
  Feed,
  NodeProblem,
  PortDirection,
  Reach,
  ResolvedNode,
  ResolvedPatch,
  ResolvedPort,
  ResolvedState,
  Runs,
  ServerEvent,
  Tap,
} from "./types";

const EMPTY: ResolvedState = { patch: { nodes: [], edges: [] }, problems: [] };
const NO_FEEDS: readonly Feed[] = [];
const NO_REACHES: readonly Reach[] = [];
const NO_KINDS: readonly EventKind[] = [];

export function withPatch(state: ResolvedState | undefined, patch: ResolvedPatch): ResolvedState {
  return { ...(state ?? EMPTY), patch };
}

export function withProblems(
  state: ResolvedState | undefined,
  problems: NodeProblem[],
): ResolvedState {
  return { ...(state ?? EMPTY), problems };
}

export function observeResolved(queryClient: QueryClient, event: ServerEvent): void {
  if (event.type === "PatchResolved") {
    queryClient.setQueryData<ResolvedState>(RESOLVED_KEY, (state) => withPatch(state, event.data));
  } else if (event.type === "NodeProblems") {
    queryClient.setQueryData<ResolvedState>(RESOLVED_KEY, (state) =>
      withProblems(state, event.data.problems),
    );
  }
}

export function activePatch(
  state: ResolvedState | undefined,
  workspace: number | null,
): ResolvedPatch | null {
  return state !== undefined && workspace !== null && state.patch.workspace === workspace
    ? state.patch
    : null;
}

export function resolvedNode(patch: ResolvedPatch | null, id: string): ResolvedNode | undefined {
  return patch?.nodes.find((node) => node.id === id);
}

export function portOn(
  patch: ResolvedPatch | null,
  node: string,
  port: string,
  direction: PortDirection,
): ResolvedPort | undefined {
  return resolvedNode(patch, node)?.ports.find(
    (candidate) => candidate.name === port && candidate.direction === direction,
  );
}

export function feedsOf(patch: ResolvedPatch | null, node: string, port: string): readonly Feed[] {
  return portOn(patch, node, port, "in")?.feeds ?? NO_FEEDS;
}

export function ignoredOf(
  patch: ResolvedPatch | null,
  node: string,
  port: string,
): readonly EventKind[] {
  return portOn(patch, node, port, "in")?.ignored ?? NO_KINDS;
}

export function feedSources(patch: ResolvedPatch | null, node: string, port: string): string[] {
  return [...new Set(feedsOf(patch, node, port).map((feed) => feed.source))];
}

export function reachesOf(
  patch: ResolvedPatch | null,
  node: string,
  port: string,
): readonly Reach[] {
  return portOn(patch, node, port, "out")?.reaches ?? NO_REACHES;
}

export function runsOf(patch: ResolvedPatch | null, node: string): Runs | undefined {
  return resolvedNode(patch, node)?.runs;
}

export function problemsOf(problems: readonly NodeProblem[], node: string): NodeProblem[] {
  return problems.filter((problem) => problem.node === node);
}

export function radioRunning(patch: ResolvedPatch | null, deviceSet: number): string | undefined {
  return patch?.nodes.find(
    (node) => node.runs.runs === "radio" && node.runs.device_set === deviceSet,
  )?.id;
}

export function deviceSetIdOf(patch: ResolvedPatch | null, node: string): number | null {
  const runs = runsOf(patch, node);
  if (runs?.runs === "radio" || runs?.runs === "channel") {
    return runs.device_set;
  }
  for (const feed of feedsOf(patch, node, "iq")) {
    if (feed.tap?.tap === "lane") {
      return feed.tap.device_set;
    }
  }
  for (const reach of reachesOf(patch, node, "control")) {
    if (reach.target_runs?.runs === "channel") {
      return reach.target_runs.device_set;
    }
  }
  return null;
}

export function runningSet(
  patch: ResolvedPatch | null,
  sets: readonly DeviceSet[],
  node: string,
): DeviceSet | null {
  const id = deviceSetIdOf(patch, node);
  return id === null ? null : (sets.find((set) => set.id === id) ?? null);
}

export interface Controlled {
  node: string;
  set: DeviceSet;
  channel: ChannelInfo;
}

export function controlledChannel(
  patch: ResolvedPatch | null,
  sets: readonly DeviceSet[],
  node: string,
): Controlled | null {
  for (const reach of reachesOf(patch, node, "control")) {
    const runs = reach.target_runs;
    if (runs?.runs !== "channel") {
      continue;
    }
    const set = sets.find((candidate) => candidate.id === runs.device_set);
    const channel = set?.channels.find((candidate) => candidate.id === runs.channel);
    if (set !== undefined && channel !== undefined) {
      return { node: reach.sink, set, channel };
    }
  }
  return null;
}

export function sweptBy(patch: ResolvedPatch | null, node: string, except?: string): boolean {
  return feedsOf(patch, node, "control").some(
    (feed) => feed.drives === "sweep" && feed.source !== except,
  );
}

export function controllerOf(patch: ResolvedPatch | null, node: string): string | null {
  const feed = feedsOf(patch, node, "control")[0];
  return feed === undefined ? null : (resolvedNode(patch, feed.source)?.label ?? feed.source);
}

export function heldByOf(patch: ResolvedPatch | null, node: string): string | null {
  const held = resolvedNode(patch, node)?.held;
  if (held?.by !== "wire") {
    return null;
  }
  return resolvedNode(patch, held.node)?.label ?? held.node;
}

export function tapChannels(tap: Tap | null | undefined): ChannelAddr[] {
  switch (tap?.tap) {
    case "channel":
      return [{ device_set: tap.device_set, channel: tap.channel }];
    case "channels":
      return tap.channels;
    default:
      return [];
  }
}

export function channelAt(sets: readonly DeviceSet[], addr: ChannelAddr): ChannelInfo | undefined {
  return sets
    .find((set) => set.id === addr.device_set)
    ?.channels.find((channel) => channel.id === addr.channel);
}

export function boundRadios(
  patch: ResolvedPatch | null,
  sets: readonly DeviceSet[],
): Map<string, DeviceSet> {
  const bound = new Map<string, DeviceSet>();
  for (const node of patch?.nodes ?? []) {
    const runs = node.runs;
    const set =
      runs.runs === "radio" ? sets.find((open) => open.id === runs.device_set) : undefined;
    if (set !== undefined) {
      bound.set(node.id, set);
    }
  }
  return bound;
}

export function boundChannels(
  patch: ResolvedPatch | null,
  sets: readonly DeviceSet[],
): Map<string, ChannelInfo> {
  const bound = new Map<string, ChannelInfo>();
  for (const node of patch?.nodes ?? []) {
    const channel = node.runs.runs === "channel" ? channelAt(sets, node.runs) : undefined;
    if (channel !== undefined) {
      bound.set(node.id, channel);
    }
  }
  return bound;
}

export function channelNodesOn(
  patch: ResolvedPatch | null,
  deviceSet: number,
): Map<number, string> {
  const nodes = new Map<number, string>();
  for (const node of patch?.nodes ?? []) {
    if (node.runs.runs === "channel" && node.runs.device_set === deviceSet) {
      nodes.set(node.runs.channel, node.id);
    }
  }
  return nodes;
}

export function channelOwners(patch: ResolvedPatch | null): Map<string, string> {
  const owners = new Map<string, string>();
  for (const node of patch?.nodes ?? []) {
    const radio = node.runs.runs === "channel" ? radioRunning(patch, node.runs.device_set) : null;
    if (radio != null) {
      owners.set(node.id, radio);
    }
  }
  return owners;
}

export interface Input {
  node: string;
  source: string;
  deviceSet: number;
  channel: ChannelInfo;
  fx: string[];
}

function feedSource(feed: Feed, index: number): string {
  return streamSource(feed.source, feed.via ?? [], index);
}

export function inputsOf(
  patch: ResolvedPatch | null,
  sets: readonly DeviceSet[],
  node: string,
  port: string,
): Input[] {
  return feedsOf(patch, node, port).flatMap((feed) =>
    tapChannels(feed.tap).flatMap((addr, index) => {
      const channel = channelAt(sets, addr);
      return channel === undefined
        ? []
        : [
            {
              node: feed.source,
              source: feedSource(feed, index),
              deviceSet: addr.device_set,
              channel,
              fx: feed.via ?? [],
            },
          ];
    }),
  );
}

export function useInputs(node: string, port: string): Input[] {
  const workspace = useWorkspaceContext();
  return inputsOf(workspace.resolved, workspace.deviceSets, node, port);
}

export function useFeeds(node: string, port: string): readonly Feed[] {
  return feedsOf(useWorkspaceContext().resolved, node, port);
}

export function useNodeProblems(node: string): NodeProblem[] {
  return problemsOf(useWorkspaceContext().problems, node);
}
