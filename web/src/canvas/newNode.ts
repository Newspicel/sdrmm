import type {
  HuntSweepParams,
  NodeBody,
  NodeKind,
  PatchCatalog,
  PatchNodeOf,
  ProcessorType,
  ScanSettings,
} from "../lib/types";

export interface NewNodeSeed {
  /** The decoder, encoder or plugin processor the node runs; a node that picks one has no
   * body without it. */
  typeId?: string;
}

export function processorTypeOf(catalog: PatchCatalog, typeId: string): ProcessorType | undefined {
  return catalog.processors?.find((type) => type.type_id === typeId);
}

/** What a fresh node of `type` starts on. */
export function processorDefaults(type: ProcessorType | undefined): Record<string, unknown> {
  const defaults = type?.defaults;
  return typeof defaults === "object" && defaults !== null
    ? structuredClone(defaults as Record<string, unknown>)
    : {};
}

export function defaultBody(catalog: PatchCatalog, kind: NodeKind): NodeBody | null {
  const entry = catalog.nodes.find((type) => type.kind === kind);
  return entry === undefined || entry.default_body.kind !== kind
    ? null
    : structuredClone(entry.default_body);
}

export function newNodeBody(
  catalog: PatchCatalog,
  kind: NodeKind,
  seed: NewNodeSeed = {},
): NodeBody | null {
  if (kind === "channel") {
    return seed.typeId === undefined ? null : { kind, data: { channel_type: seed.typeId } };
  }
  if (kind === "encoder") {
    return seed.typeId === undefined ? null : { kind, data: { encoder_type: seed.typeId } };
  }
  if (kind === "processor") {
    const type = processorTypeOf(catalog, seed.typeId ?? "");
    return type === undefined
      ? null
      : { kind, data: { processor_type: type.type_id, settings: processorDefaults(type) } };
  }
  return defaultBody(catalog, kind);
}

export function carriesSettings(catalog: PatchCatalog, kind: NodeKind): boolean {
  const body = defaultBody(catalog, kind);
  return body !== null && "data" in body;
}

export function huntSweepOf(
  node: PatchNodeOf<"hunt">,
  catalog: PatchCatalog,
): HuntSweepParams | null {
  if (node.data?.sweep !== undefined) {
    return node.data.sweep;
  }
  const body = defaultBody(catalog, "hunt");
  return body?.kind === "hunt" ? (body.data?.sweep ?? null) : null;
}

export function defaultScannerSettings(catalog: PatchCatalog): ScanSettings {
  const body = defaultBody(catalog, "scanner");
  const settings = body?.kind === "scanner" ? body.data?.settings : undefined;
  return settings ?? { ranges: [] };
}
