import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import { deviceSet } from "../test/fixtures";
import { resolvedNodeOf, resolvedPatch, resolvedPort } from "../test/resolved";
import { RESOLVED_KEY } from "./api";
import {
  activePatch,
  boundChannels,
  boundRadios,
  channelOwners,
  controlledChannel,
  feedsOf,
  heldByOf,
  ignoredOf,
  inputsOf,
  observeResolved,
  reachesOf,
  runningSet,
  withPatch,
  withProblems,
} from "./resolved";
import type { ChannelInfo, ResolvedState } from "./types";

function channel(id: number, node: string, stream = 0): ChannelInfo {
  return {
    id,
    node,
    stream,
    settings: { frequency_hz: 100_000_000, params: { type: "nfm", settings: {} } as never },
  };
}

const iqIn = (source: string, stream = 0) =>
  resolvedPort(
    { name: "iq", port_type: "iq", direction: "in", multi: "many" },
    { feeds: [{ source, port: "iq", tap: { tap: "lane", device_set: 1, stream } }] },
  );

function patch() {
  return resolvedPatch([
    resolvedNodeOf(
      "dev",
      "device",
      [
        resolvedPort(
          { name: "iq", port_type: "iq", direction: "out", multi: "many" },
          { reaches: [{ sink: "nfm", port: "iq" }] },
        ),
      ],
      { runs: "radio", device_set: 1 },
    ),
    resolvedNodeOf(
      "nfm",
      "channel",
      [
        iqIn("dev"),
        resolvedPort(
          { name: "audio", port_type: "audio", direction: "out", multi: "many" },
          { reaches: [{ sink: "spk", port: "audio", via: ["fx"] }] },
        ),
      ],
      { runs: "channel", device_set: 1, channel: 4, stream: 0 },
    ),
    resolvedNodeOf("spk", "speaker", [
      resolvedPort(
        { name: "audio", port_type: "audio", direction: "in", multi: "many" },
        {
          feeds: [
            {
              source: "nfm",
              port: "audio",
              via: ["fx"],
              tap: { tap: "channel", device_set: 1, channel: 4 },
            },
            {
              source: "trunk",
              port: "audio",
              tap: {
                tap: "channels",
                channels: [
                  { device_set: 1, channel: 6 },
                  { device_set: 1, channel: 99 },
                ],
              },
            },
          ],
        },
      ),
    ]),
    resolvedNodeOf(
      "scan",
      "scanner",
      [
        resolvedPort(
          { name: "control", port_type: "control", direction: "out" },
          {
            reaches: [
              { sink: "spk", port: "audio" },
              {
                sink: "nfm",
                port: "control",
                target_runs: { runs: "channel", device_set: 1, channel: 4, stream: 0 },
              },
            ],
          },
        ),
      ],
      { runs: "server" },
    ),
    resolvedNodeOf("lost", "scanner", [], { runs: "server" }),
  ]);
}

const live = [deviceSet({ id: 1, channels: [channel(4, "nfm"), channel(6, "trunk")] })];

describe("the resolved store", () => {
  it("applies a resolved patch and problems as they arrive", () => {
    const client = new QueryClient();
    observeResolved(client, { type: "PatchResolved", data: patch() });
    expect(client.getQueryData<ResolvedState>(RESOLVED_KEY)?.patch.nodes).toHaveLength(5);
    const problem = { node: "dev", source: "bind" as const, text: "Radio not open" };
    observeResolved(client, { type: "NodeProblems", data: { problems: [problem] } });
    const state = client.getQueryData<ResolvedState>(RESOLVED_KEY);
    expect(state?.problems).toEqual([problem]);
    expect(state?.patch.nodes).toHaveLength(5);
    observeResolved(client, { type: "Hello", data: {} } as never);
    expect(client.getQueryData<ResolvedState>(RESOLVED_KEY)).toBe(state);
  });

  it("keeps problems when a patch arrives and the patch when problems do", () => {
    const first = withProblems(undefined, [{ node: "a", source: "apply", text: "No" }]);
    const next = withPatch(first, patch());
    expect(next.problems).toEqual(first.problems);
    expect(withProblems(next, []).patch).toBe(next.patch);
  });

  it("serves only the active workspace's patch", () => {
    const state = withPatch(undefined, patch());
    expect(activePatch(state, 1)).toBe(state.patch);
    expect(activePatch(state, 2)).toBeNull();
    expect(activePatch(state, null)).toBeNull();
    expect(activePatch(undefined, 1)).toBeNull();
  });
});

describe("resolved lookups", () => {
  it("indexes live state at the address each node runs", () => {
    const resolved = patch();
    expect(boundRadios(resolved, live).get("dev")?.id).toBe(1);
    expect(boundRadios(resolved, []).size).toBe(0);
    expect(boundChannels(resolved, live).get("nfm")?.id).toBe(4);
    expect(channelOwners(resolved)).toEqual(new Map([["nfm", "dev"]]));
  });

  it("reads feeds and reaches by port", () => {
    const resolved = patch();
    expect(feedsOf(resolved, "nfm", "iq").map((feed) => feed.source)).toEqual(["dev"]);
    expect(feedsOf(resolved, "nfm", "audio")).toEqual([]);
    expect(reachesOf(resolved, "nfm", "audio").map((reach) => reach.sink)).toEqual(["spk"]);
    expect(feedsOf(null, "nfm", "iq")).toEqual([]);
  });

  it("names the node a decoder is held by, never a hand lock", () => {
    const resolved = resolvedPatch([
      { ...resolvedNodeOf("nfm", "channel"), held: { by: "wire", node: "scan" } },
      { ...resolvedNodeOf("am", "channel"), held: { by: "hand" } },
      { ...resolvedNodeOf("scan", "scanner"), label: "Scanner" },
    ]);
    expect(heldByOf(resolved, "nfm")).toBe("Scanner");
    expect(heldByOf(resolved, "am")).toBeNull();
    expect(heldByOf(null, "nfm")).toBeNull();
  });

  it("names the kinds an input ignores", () => {
    const map = resolvedPatch([
      resolvedNodeOf("map", "map", [
        {
          ...resolvedPort({ name: "events", port_type: "events", direction: "in" }),
          ignored: ["call"],
        },
      ]),
    ]);
    expect(ignoredOf(map, "map", "events")).toEqual(["call"]);
    expect(ignoredOf(map, "map", "position")).toEqual([]);
    expect(ignoredOf(null, "map", "events")).toEqual([]);
  });

  it("names every audio stream a port plays as the server does", () => {
    expect(inputsOf(patch(), live, "spk", "audio").map((input) => input.source)).toEqual([
      "nfm/fx",
      "trunk",
    ]);
  });

  it("reads where a node runs from what the server served", () => {
    const resolved = patch();
    for (const node of ["dev", "nfm", "scan"]) {
      expect(runningSet(resolved, live, node)?.id).toBe(1);
    }
    expect(runningSet(resolved, live, "lost")).toBeNull();
    expect(runningSet(resolved, live, "spk")).toBeNull();
    expect(controlledChannel(resolved, live, "scan")).toMatchObject({
      node: "nfm",
      channel: { id: 4 },
    });
    expect(controlledChannel(resolved, live, "lost")).toBeNull();
  });
});
