import type { ExampleInfo, PatchCatalog } from "../lib/types";

function plural(count: number, one: string, many: string): string {
  return `${count} ${count === 1 ? one : many}`;
}

export function exampleNeeds(example: ExampleInfo): string {
  const { radios, lanes, recordings } = example.needs;
  if (lanes > 1) return plural(lanes, "lane", "lanes");
  if (radios > 0) return plural(radios, "radio", "radios");
  if (recordings > 0) return plural(recordings, "recording", "recordings");
  return "no radio";
}

export function examplesHint(pending: boolean, error: Error | null): string | null {
  if (error !== null) return error.message || "The examples could not be read.";
  if (pending) return "Loading examples.";
  return null;
}

export function kindLabels(example: ExampleInfo, catalog: PatchCatalog | undefined): string[] {
  return example.node_kinds.map(
    (kind) => catalog?.nodes.find((type) => type.kind === kind)?.name ?? kind,
  );
}
