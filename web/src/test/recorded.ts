import { mkdirSync, writeFileSync } from "node:fs";
import { expect } from "vitest";
import fixtures from "../generated/resolved-fixtures.json" with { type: "json" };
import type { DeviceSet, PatchGraph, ResolvedPatch } from "../lib/types";
import { resolvedPatch } from "./resolved";

const INPUTS = new URL("./resolved-inputs/", import.meta.url);
const recording = import.meta.env.SDRMM_RECORD_RESOLVED === "1";
const FNV_OFFSET = 0xcbf29ce484222325n;
const FNV_PRIME = 0x100000001b3n;
const MASK = 0xffffffffffffffffn;

const RECORDED = fixtures as unknown as Record<string, { patch: ResolvedPatch }>;

function hashOf(text: string): string {
  let hash = FNV_OFFSET;
  for (let index = 0; index < text.length; index += 1) {
    hash = ((hash ^ BigInt(text.charCodeAt(index))) * FNV_PRIME) & MASK;
  }
  return hash.toString(16).padStart(16, "0");
}

export function resolvedBy(
  graph: PatchGraph,
  devices: ReadonlyMap<string, DeviceSet> = new Map(),
): ResolvedPatch {
  const input = { graph, live: { device_sets: [...devices.values()], revision: 0 } };
  const text = JSON.stringify(input);
  const name = hashOf(text);
  const recorded = RECORDED[name];
  if (recording) {
    mkdirSync(INPUTS, { recursive: true });
    const test = expect.getState().currentTestName ?? "";
    writeFileSync(
      new URL(`${name}.json`, INPUTS),
      `${JSON.stringify({ name, test, ...JSON.parse(text) }, null, 2)}\n`,
    );
    return recorded?.patch ?? resolvedPatch([]);
  }
  if (recorded === undefined) {
    throw new Error("No resolved fixture for this graph: run pnpm record:resolved");
  }
  return recorded.patch;
}
