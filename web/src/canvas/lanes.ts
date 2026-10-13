import { channelAt, feedsOf, radioRunning, resolvedNode, tapChannels } from "../lib/resolved";
import type { ChannelInfo, DeviceSet, Feed, PortRef, ResolvedPatch } from "../lib/types";
import { opensDevice } from "./graph";

export interface VirtualSource {
  node: string;
  port: string;
  array: string | null;
}

export interface IqLane {
  source: string;
  stream: number;
  virtual?: VirtualSource;
}

export function arrayOf(patch: ResolvedPatch | null, processor: string): string | null {
  return feedsOf(patch, processor, "array")[0]?.source ?? null;
}

export function hasFeed(patch: ResolvedPatch | null, node: string, port: string): boolean {
  return feedsOf(patch, node, port).length > 0;
}

function laneOfFeed(patch: ResolvedPatch | null, feed: Feed): IqLane | null {
  const source = resolvedNode(patch, feed.source);
  if (feed.tap?.tap !== "lane") {
    return null;
  }
  if (source !== undefined && opensDevice(source.kind)) {
    return { source: feed.source, stream: feed.tap.stream };
  }
  const radio = radioRunning(patch, feed.tap.device_set);
  return radio === undefined
    ? null
    : {
        source: radio,
        stream: feed.tap.stream,
        virtual: { node: feed.source, port: feed.port, array: arrayOf(patch, feed.source) },
      };
}

export function iqLanesOf(patch: ResolvedPatch | null, node: string): IqLane[] {
  return feedsOf(patch, node, "iq").flatMap((feed) => laneOfFeed(patch, feed) ?? []);
}

export function iqSourceOf(patch: ResolvedPatch | null, node: string): IqLane | null {
  return iqLanesOf(patch, node)[0] ?? null;
}

export interface BasebandSource {
  node: string;
  deviceSet: number;
  channel: ChannelInfo;
}

export function basebandSourceOf(
  patch: ResolvedPatch | null,
  sets: readonly DeviceSet[],
  node: string,
): BasebandSource | null {
  for (const feed of feedsOf(patch, node, "baseband")) {
    for (const addr of tapChannels(feed.tap)) {
      const channel = channelAt(sets, addr);
      if (channel !== undefined) {
        return { node: feed.source, deviceSet: addr.device_set, channel };
      }
    }
  }
  return null;
}

export interface ArrayLane {
  lane: number;
  port: string;
  source: PortRef;
  stream: number;
}

export function arrayLanes(resolved: ResolvedPatch | null, array: string): ArrayLane[] {
  return (resolvedNode(resolved, array)?.ports ?? [])
    .flatMap((port) => {
      const lane = port.direction === "in" && port.port_type === "iq" ? port.stream : undefined;
      const feed = port.feeds?.[0];
      return lane == null || feed === undefined
        ? []
        : [
            {
              lane,
              port: port.name,
              source: { node: feed.source, port: feed.port },
              stream: feed.stream ?? 0,
            },
          ];
    })
    .toSorted((a, b) => a.lane - b.lane);
}

export function wiredLaneCount(resolved: ResolvedPatch | null, array: string): number {
  return Math.max(0, ...arrayLanes(resolved, array).map((wire) => wire.lane + 1));
}
