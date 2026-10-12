import { describe, expect, it } from "vitest";
import { CATALOG } from "./test/catalog";

const SOURCES: Record<string, string> = import.meta.glob("./**/*.{ts,tsx}", {
  query: "?raw",
  import: "default",
  eager: true,
});

const EDGE_EDITORS: ReadonlySet<string> = new Set([
  "./canvas/Canvas.tsx",
  "./canvas/clipboard.ts",
  "./canvas/graph.ts",
  "./components/report.ts",
]);

const EDGE_READERS_UNTIL_LATER_SLICES: ReadonlySet<string> = new Set([
  "./canvas/nodes/decoderSwap.ts",
]);

const EDGES = /\.edges\b/;
const FIXTURE = /generated\/catalog-fixture\.json/;

function product(path: string): boolean {
  return (
    !/\.test\.tsx?$/.test(path) && !path.startsWith("./test/") && path !== "./generated/schema.d.ts"
  );
}

function offenders(pattern: RegExp, allowed: ReadonlySet<string>): string[] {
  return Object.entries(SOURCES)
    .filter(([path, source]) => product(path) && !allowed.has(path) && pattern.test(source))
    .map(([path]) => path)
    .toSorted();
}

describe("node contracts", () => {
  it("reads wires only where the graph is edited", () => {
    expect(
      offenders(EDGES, new Set([...EDGE_EDITORS, ...EDGE_READERS_UNTIL_LATER_SLICES])),
    ).toEqual([]);
  });

  it("takes the catalog from the server, never from the test fixture", () => {
    expect(offenders(FIXTURE, new Set())).toEqual([]);
  });

  it("leaves wire rules to the server", () => {
    expect(Object.keys(SOURCES).filter((path) => path.endsWith("/arrayRules.ts"))).toEqual([]);
    expect(offenders(/connectionRefusal|arrayRefusal|BEAM_TAKES_ONE/, new Set())).toEqual([]);
  });

  it("binds nothing in the client", () => {
    expect(Object.keys(SOURCES).filter((path) => path.endsWith("/binding.ts"))).toEqual([]);
    expect(offenders(/positionSourcesOf|bindDevices|bindCarriers/, new Set())).toEqual([]);
  });

  it("names no event kind in the generic sinks and the map", () => {
    const kinds = CATALOG.event_kinds.map((entry) => entry.kind);
    const literal = new RegExp(`["'\`](?:${kinds.join("|")})["'\`]`);
    const generic = Object.entries(SOURCES)
      .filter(
        ([path]) =>
          product(path) && (path.startsWith("./sinks/") || path.startsWith("./components/map/")),
      )
      .filter(([, source]) => literal.test(source))
      .map(([path]) => path);
    expect(generic).toEqual([]);
  });
});
