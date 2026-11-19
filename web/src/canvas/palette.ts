import type {
  ChannelDescriptor,
  DecoderFamily,
  EncoderDescriptor,
  FamilyEntry,
  NodeCategory,
  NodeKind,
  NodeTypeInfo,
  PatchCatalog,
} from "../lib/types";

export type PaletteSection = NodeCategory;

export interface PaletteItem {
  id: string;
  name: string;
  summary: string;
  kind: NodeKind;
  /** The decoder, encoder or plugin processor the node runs. */
  typeId?: string;
  /** The width the type occupies, shown beside its name. */
  bandwidthHz?: number;
}

export interface PaletteGroup {
  id: string;
  title: string;
  section: PaletteSection;
  items: PaletteItem[];
}

export const SECTION_TITLES = {
  source: "Sources",
  channel: "Decoders",
  encoder: "Encoders",
  tool: "Tools",
  output: "Outputs",
} satisfies Record<PaletteSection, string>;

export const SECTIONS: readonly { id: PaletteSection; title: string }[] = Object.entries(
  SECTION_TITLES,
).map(([id, title]) => ({ id: id as PaletteSection, title }));

export function familyOf(type: ChannelDescriptor): DecoderFamily {
  return type.family ?? "utility";
}

/** The server's groups in its order, then any it did not list, under their id. */
export function familiesOf(
  families: readonly FamilyEntry[],
  types: readonly ChannelDescriptor[],
): FamilyEntry[] {
  const listed = new Set(families.map((family) => family.id));
  const unlisted = new Set(types.map(familyOf).filter((family) => !listed.has(family)));
  return [...families, ...[...unlisted].toSorted().map((id) => ({ id, title: id }))];
}

export function paletteGroups(
  catalog: PatchCatalog,
  channelTypes: readonly ChannelDescriptor[],
  encoderTypes: readonly EncoderDescriptor[] = [],
): PaletteGroup[] {
  const sections = new Map<PaletteSection, PaletteItem[]>(
    SECTIONS.map((section) => [section.id, []]),
  );
  for (const entry of catalog.nodes) {
    sections.get(entry.category)?.push(...entryItems(catalog, entry, encoderTypes));
  }
  const offersDecoders = catalog.nodes.some((entry) => entry.picks === "decoder");
  return SECTIONS.flatMap((section) => {
    const plain = { ...section, section: section.id, items: sections.get(section.id) ?? [] };
    return section.id === "channel" && offersDecoders
      ? [plain, ...decoderGroups(catalog.families ?? [], channelTypes)]
      : [plain];
  }).filter((group) => group.items.length > 0);
}

/** What one catalog entry adds: itself, or one item per type of the registry it picks from. */
function entryItems(
  catalog: PatchCatalog,
  entry: NodeTypeInfo,
  encoderTypes: readonly EncoderDescriptor[],
): PaletteItem[] {
  switch (entry.picks) {
    case "decoder":
      return [];
    case "encoder":
      return encoderTypes.map((type) => typeItem(entry.kind, type, type.bandwidth_hz));
    case "processor":
      return (catalog.processors ?? []).map((type) => typeItem(entry.kind, type));
    default:
      return [{ id: entry.kind, name: entry.name, summary: entry.summary ?? "", kind: entry.kind }];
  }
}

function typeItem(
  kind: NodeKind,
  type: { type_id: string; name: string; summary?: string },
  bandwidthHz?: number,
): PaletteItem {
  return {
    id: `${kind}:${type.type_id}`,
    name: type.name,
    summary: type.summary ?? "",
    kind,
    typeId: type.type_id,
    bandwidthHz,
  };
}

export function decoderGroups(
  families: readonly FamilyEntry[],
  channelTypes: readonly ChannelDescriptor[],
): PaletteGroup[] {
  return familiesOf(families, channelTypes)
    .map((family) => ({
      id: `family:${family.id}`,
      title: family.title,
      section: "channel" as const,
      items: channelItems(channelTypes.filter((type) => familyOf(type) === family.id)),
    }))
    .filter((group) => group.items.length > 0);
}

function channelItems(channelTypes: readonly ChannelDescriptor[]): PaletteItem[] {
  return channelTypes.map((type) => typeItem("channel", type, type.bandwidth_hz));
}

export function channelPicker(
  families: readonly FamilyEntry[],
  channelTypes: readonly ChannelDescriptor[],
  suggested: string | undefined,
): PaletteGroup[] {
  const groups = decoderGroups(families, channelTypes);
  const type = channelTypes.find((entry) => entry.type_id === suggested);
  if (type === undefined) {
    return groups;
  }
  return [
    {
      id: "suggested",
      title: "Suggested",
      section: "channel",
      items: channelItems([type]).map((item) => ({ ...item, id: `suggested:${type.type_id}` })),
    },
    ...groups,
  ];
}

export function decoderReplacements(
  families: readonly FamilyEntry[],
  channelTypes: readonly ChannelDescriptor[],
  current: string,
): PaletteGroup[] {
  return decoderGroups(
    families,
    channelTypes.filter((type) => type.type_id !== current),
  );
}

export function firstPaletteItem(groups: readonly PaletteGroup[]): PaletteItem | undefined {
  return groups[0]?.items[0];
}

export function sectionGroups(
  groups: readonly PaletteGroup[],
  section: PaletteSection,
): PaletteGroup[] {
  return groups.filter((group) => group.section === section);
}

export function filterPalette(groups: readonly PaletteGroup[], query: string): PaletteGroup[] {
  const needle = query.trim().toLowerCase();
  if (needle === "") {
    return [...groups];
  }
  return groups
    .map((group) => ({ ...group, items: group.items.filter((item) => matches(item, needle)) }))
    .filter((group) => group.items.length > 0);
}

function matches(item: PaletteItem, needle: string): boolean {
  return [item.name, item.id, item.summary].some((text) => text.toLowerCase().includes(needle));
}
