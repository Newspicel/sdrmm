import type {
  ChannelDescriptor,
  DecoderFamily,
  NodeCategory,
  NodeKind,
  PatchCatalog,
} from "../lib/types";

export type PaletteSection = NodeCategory;

export interface PaletteItem {
  id: string;
  name: string;
  summary: string;
  kind: NodeKind;
  type?: ChannelDescriptor;
  /** The plugin processor a processor node runs. */
  processorType?: string;
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
  tool: "Tools",
  output: "Outputs",
} satisfies Record<PaletteSection, string>;

export const SECTIONS: readonly { id: PaletteSection; title: string }[] = Object.entries(
  SECTION_TITLES,
).map(([id, title]) => ({ id: id as PaletteSection, title }));

export const FAMILIES: Readonly<Record<string, string>> = {
  analog_voice: "Analog voice",
  digital_voice: "Digital voice",
  aviation: "Aviation",
  marine: "Marine",
  amateur: "Amateur and HF",
  paging: "Paging and telemetry",
  video: "Pictures and video",
  broadcast: "Broadcast digital",
  weather: "Weather and satellites",
  utility: "Utility",
};

export function familyTitle(family: DecoderFamily): string {
  return FAMILIES[family] ?? family.charAt(0).toUpperCase() + family.slice(1).replaceAll("_", " ");
}

export function familyOf(type: ChannelDescriptor): DecoderFamily {
  return type.family ?? "utility";
}

/** Built-in families in their set order, then any a plugin brought, by name. */
export function familiesOf(types: readonly ChannelDescriptor[]): DecoderFamily[] {
  const known = Object.keys(FAMILIES);
  const added = new Set(types.map(familyOf).filter((family) => !known.includes(family)));
  return [...known, ...[...added].toSorted()];
}

export function paletteGroups(
  catalog: PatchCatalog,
  channelTypes: readonly ChannelDescriptor[],
): PaletteGroup[] {
  const sections = new Map<PaletteSection, PaletteItem[]>(
    SECTIONS.map((section) => [section.id, []]),
  );
  for (const entry of catalog.nodes) {
    if (entry.needs_processor_type === true) {
      sections.get(entry.category)?.push(...processorItems(catalog, entry.kind));
    } else if (entry.needs_channel_type !== true) {
      sections.get(entry.category)?.push({
        id: entry.kind,
        name: entry.name,
        summary: entry.summary ?? "",
        kind: entry.kind,
      });
    }
  }
  const offersChannels = catalog.nodes.some((entry) => entry.needs_channel_type === true);
  return SECTIONS.flatMap((section) =>
    section.id === "channel"
      ? offersChannels
        ? decoderGroups(channelTypes)
        : []
      : [{ ...section, section: section.id, items: sections.get(section.id) ?? [] }],
  ).filter((group) => group.items.length > 0);
}

function processorItems(catalog: PatchCatalog, kind: NodeKind): PaletteItem[] {
  return (catalog.processors ?? []).map((type) => ({
    id: `processor:${type.type_id}`,
    name: type.name,
    summary: type.summary ?? "",
    kind,
    processorType: type.type_id,
  }));
}

export function decoderGroups(channelTypes: readonly ChannelDescriptor[]): PaletteGroup[] {
  return familiesOf(channelTypes)
    .map((family) => ({
      id: `family:${family}`,
      title: familyTitle(family),
      section: "channel" as const,
      items: channelItems(channelTypes.filter((type) => familyOf(type) === family)),
    }))
    .filter((group) => group.items.length > 0);
}

function channelItems(channelTypes: readonly ChannelDescriptor[]): PaletteItem[] {
  return channelTypes.map((type) => ({
    id: `channel:${type.type_id}`,
    name: type.name,
    summary: type.summary ?? "",
    kind: "channel",
    type,
  }));
}

export function channelPicker(
  channelTypes: readonly ChannelDescriptor[],
  suggested: string | undefined,
): PaletteGroup[] {
  const groups = decoderGroups(channelTypes);
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
  channelTypes: readonly ChannelDescriptor[],
  current: string,
): PaletteGroup[] {
  return decoderGroups(channelTypes.filter((type) => type.type_id !== current));
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
