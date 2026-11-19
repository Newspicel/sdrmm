import { formatHz } from "../../components/format";
import type {
  ChannelDescriptor,
  SettingField,
  TrunkBand,
  TrunkChannel,
  TrunkChannelEntry,
  TrunkChannelSource,
  TrunkFollower,
  TrunkScheme,
  TrunkSearchRange,
  TrunkSite,
  TrunkSpec,
  TrunkSystemStatus,
  VoiceKey,
} from "../../lib/types";

export const MAX_SEARCH_RANGES = 8;
export const MAX_SEARCH_CANDIDATES = 512;
export const MIN_SEARCH_STEP_HZ = 1_250;
export const MAX_CHANNEL_MAP = 512;
export const MAX_TALKGROUPS = 256;
export const AUTO_PROTOCOL = "auto";
export const DEFAULT_BAND_SPACING_HZ = 12_500;
export const MIN_BAND_SPACING_HZ = 1_250;
export const MAX_BAND_SPACING_HZ = 1_000_000;

/** The decoders a trunk system can run on, in registry order. */
export function trunkDecoders(
  types: readonly ChannelDescriptor[],
): { value: string; label: string }[] {
  return types
    .filter((type) => type.trunking != null)
    .map((type) => ({ value: type.type_id, label: type.name }));
}

/** The node's decoder: its own pick, or the first decoder that can trunk. */
function trunkDecoder(
  types: readonly ChannelDescriptor[],
  decoder: string,
): ChannelDescriptor | undefined {
  return types.find(
    (type) => type.trunking != null && (decoder === "" || type.type_id === decoder),
  );
}

/** How the node's decoder trunks. */
export function trunkSpec(types: readonly ChannelDescriptor[], decoder: string): TrunkSpec | null {
  return trunkDecoder(types, decoder)?.trunking ?? null;
}

/** The highest logical channel the protocol numbers, or any channel while none is known. */
export function maxChannel(spec: TrunkSpec | null): number {
  return spec?.max_channel ?? 65_535;
}

export function trunkProtocols(spec: TrunkSpec | null): { value: string; label: string }[] {
  return [
    { value: AUTO_PROTOCOL, label: "Auto-detect" },
    ...(spec?.schemes ?? []).map((scheme) => ({ value: scheme.id, label: scheme.label })),
  ];
}

/** The protocol the node runs: the one picked, or the one the signalling named. */
export function activeScheme(
  spec: TrunkSpec | null,
  protocol: string,
  detected: string | null = null,
): TrunkScheme | null {
  const id = protocol === AUTO_PROTOCOL ? detected : protocol;
  return spec?.schemes.find((scheme) => scheme.id === id) ?? null;
}

export function trunkProtocolLabel(scheme: TrunkScheme | null): string {
  return scheme?.label ?? "Listening for signalling";
}

export function planLabel(scheme: TrunkScheme | null): string {
  return scheme?.kind === "grants" ? "Channel plan" : "Repeater outputs";
}

export function parseTalkgroups(text: string): number[] {
  const ids = text
    .split(/[\s,;]+/)
    .filter((part) => /^\d+$/.test(part))
    .map(Number)
    .filter((id) => Number.isSafeInteger(id) && id <= 0xffff_ffff);
  return [...new Set(ids)].slice(0, MAX_TALKGROUPS);
}

export function formatTalkgroups(ids: readonly number[] | undefined): string {
  return (ids ?? []).join(", ");
}

/** Voice keys typed as `key ID:hex key`, comma separated; a later entry for an ID wins. */
/** The decoder's own key list, so a trunk takes keys the way its channels do. */
export function trunkKeysField(
  types: readonly ChannelDescriptor[],
  decoder: string,
): SettingField | null {
  return trunkDecoder(types, decoder)?.settings?.find((field) => field.key === "keys") ?? null;
}

/** List entries as voice keys, dropping any not yet whole. */
export function voiceKeys(entries: readonly Readonly<Record<string, unknown>>[]): VoiceKey[] {
  return entries.flatMap(({ key_id, key }) =>
    typeof key_id === "number" && typeof key === "string" ? [{ key_id, key }] : [],
  );
}

/** Where a call runs: the slot only when a carrier holds more than one call. */
export function slotLabel(slot: number | null | undefined, slots: number): string {
  return slots > 1 && slot != null ? `TS ${slot}` : "Call";
}

export function followerCall(follower: TrunkFollower): string {
  const parts = [formatHz(follower.freq_hz)];
  if (follower.logical_channel != null) {
    parts.push(`LCN ${follower.logical_channel}`);
  }
  if (follower.destination != null) {
    parts.push(`${follower.group_call === false ? "ID" : "TG"} ${follower.destination}`);
  }
  return parts.join(" · ");
}

export function siteLabel(site: TrunkSite | undefined): string | null {
  const parts = [
    site?.network_id == null ? null : `net ${site.network_id}`,
    site?.site_id == null ? null : `site ${site.site_id}`,
    site?.system_id == null ? null : `sys ${site.system_id}`,
  ].filter((part): part is string => part !== null);
  return parts.length === 0 ? null : parts.join(" · ");
}

/** The control frequency the node is listening on, when it had to leave the one set. */
export function fallbackControl(
  status: TrunkSystemStatus | undefined,
  controlHz: number | null | undefined,
): number | null {
  const tuned = status?.control?.freq_hz;
  return tuned != null && controlHz != null && tuned !== controlHz ? tuned : null;
}

function megahertz(value: number): string {
  return (value / 1e6).toFixed(4).replace(/\.?0+$/, "");
}

function lines(text: string): string[] {
  return text.split(/[\n;,]/);
}

export function parseSearchRanges(text: string): TrunkSearchRange[] {
  const ranges: TrunkSearchRange[] = [];
  for (const line of lines(text)) {
    const match = /^\s*([\d.]+)\s*[-–]\s*([\d.]+)\s*\/\s*([\d.]+)\s*$/.exec(line);
    if (match === null) {
      continue;
    }
    const range = {
      start_hz: Math.round(Number(match[1]) * 1e6),
      end_hz: Math.round(Number(match[2]) * 1e6),
      step_hz: Math.round(Number(match[3]) * 1e3),
    };
    if (searchRangeValid(range)) {
      ranges.push(range);
    }
    if (ranges.length >= MAX_SEARCH_RANGES) {
      break;
    }
  }
  return ranges;
}

export function searchRangeValid(range: TrunkSearchRange): boolean {
  return (
    Number.isFinite(range.start_hz) &&
    Number.isFinite(range.end_hz) &&
    range.start_hz > 0 &&
    range.end_hz >= range.start_hz &&
    range.step_hz >= MIN_SEARCH_STEP_HZ &&
    searchCandidates([range]) <= MAX_SEARCH_CANDIDATES
  );
}

export function searchCandidates(ranges: readonly TrunkSearchRange[]): number {
  return ranges.reduce(
    (total, range) =>
      total +
      (range.step_hz > 0 ? Math.floor((range.end_hz - range.start_hz) / range.step_hz) + 1 : 0),
    0,
  );
}

export function formatSearchRanges(ranges: readonly TrunkSearchRange[] | undefined): string {
  return (ranges ?? [])
    .map(
      (range) => `${megahertz(range.start_hz)}-${megahertz(range.end_hz)} / ${range.step_hz / 1e3}`,
    )
    .join("; ");
}

export function channelEntry(
  lcn: number | null,
  mhz: number | null,
  max: number,
): TrunkChannelEntry | null {
  if (lcn === null || mhz === null || !Number.isInteger(lcn)) {
    return null;
  }
  const freq_hz = Math.round(mhz * 1e6);
  if (lcn < 0 || lcn > max || !Number.isFinite(freq_hz) || freq_hz <= 0) {
    return null;
  }
  return { lcn, freq_hz };
}

export function withChannel(
  entries: readonly TrunkChannelEntry[] | undefined,
  entry: TrunkChannelEntry,
): TrunkChannelEntry[] {
  const kept = withoutChannel(entries, entry.lcn);
  if (kept.length >= MAX_CHANNEL_MAP) {
    return kept;
  }
  return [...kept, entry].toSorted((a, b) => a.lcn - b.lcn);
}

export function parseControlHz(text: string): number | undefined {
  const value = Number(text.trim());
  if (!Number.isFinite(value) || value <= 0) {
    return undefined;
  }
  return Math.round(value * 1e6);
}

/** The band with channel 0 typed in MHz; blank or nonsense drops the band. */
export function withBandBase(band: TrunkBand | null | undefined, text: string): TrunkBand | null {
  const base_hz = parseControlHz(text);
  if (base_hz === undefined) {
    return null;
  }
  return { base_hz, spacing_hz: band?.spacing_hz ?? DEFAULT_BAND_SPACING_HZ };
}

/** The band with its spacing typed in kHz; a spacing out of range leaves it as it was. */
export function withBandSpacing(band: TrunkBand, text: string): TrunkBand {
  const spacing_hz = Math.round(Number(text.trim()) * 1e3);
  const inRange =
    Number.isFinite(spacing_hz) &&
    spacing_hz >= MIN_BAND_SPACING_HZ &&
    spacing_hz <= MAX_BAND_SPACING_HZ;
  return inRange ? { ...band, spacing_hz } : band;
}

export function controlChannelStalled(
  onIq: boolean,
  controlHz: number | null | undefined,
  carriers: number | undefined,
): boolean {
  return onIq && controlHz != null && carriers === 0;
}

export function awaitingControlChannel(
  onIq: boolean,
  controlHz: number | null | undefined,
): boolean {
  return onIq && controlHz == null;
}

export function trunkChannelSourceLabel(source: TrunkChannelSource): string {
  switch (source) {
    case "announced":
      return "announced";
    case "manual":
      return "entered";
    case "learned":
      return "found";
    case "predicted":
      return "guessed";
  }
}

export function trunkChannelSourceHint(source: TrunkChannelSource): string {
  switch (source) {
    case "announced":
      return "The system broadcast this frequency itself.";
    case "manual":
      return "You entered this frequency.";
    case "learned":
      return "A call answered a grant here, so the frequency is confirmed.";
    case "predicted":
      return "Worked out from the channel spacing of the channels already known. Never followed until a call confirms it.";
  }
}

export function trunkChannelSourceTone(source: TrunkChannelSource): string {
  return source === "predicted" ? "text-ink-faint" : "text-ink-dim";
}

export function usable(source: TrunkChannelSource): boolean {
  return source !== "predicted";
}

export function channelPlanRows(
  map: readonly TrunkChannel[],
  entries: readonly TrunkChannelEntry[] | undefined,
): TrunkChannel[] {
  const known = new Map(map.map((channel) => [channel.logical_channel, channel]));
  for (const entry of entries ?? []) {
    if (!known.has(entry.lcn)) {
      known.set(entry.lcn, {
        logical_channel: entry.lcn,
        freq_hz: entry.freq_hz,
        source: "manual",
        confidence: 100,
      });
    }
  }
  return [...known.values()].toSorted((a, b) => a.logical_channel - b.logical_channel);
}

export function withoutChannel(
  entries: readonly TrunkChannelEntry[] | undefined,
  lcn: number,
): TrunkChannelEntry[] {
  return (entries ?? []).filter((entry) => entry.lcn !== lcn);
}

export function planSummary(rows: readonly TrunkChannel[]): string {
  if (rows.length === 0) {
    return "No logical channels placed yet.";
  }
  const counted = rows.reduce<Record<string, number>>((totals, row) => {
    totals[row.source] = (totals[row.source] ?? 0) + 1;
    return totals;
  }, {});
  const parts = (["announced", "manual", "learned", "predicted"] as const)
    .filter((source) => counted[source] !== undefined)
    .map((source) => `${counted[source]} ${trunkChannelSourceLabel(source)}`);
  return `${rows.length} logical channel${rows.length === 1 ? "" : "s"}: ${parts.join(", ")}.`;
}

export function adoptable(
  map: readonly TrunkChannel[],
  entries: readonly TrunkChannelEntry[] | undefined,
): TrunkChannelEntry[] {
  const known = new Set((entries ?? []).map((entry) => entry.lcn));
  return map
    .filter((channel) => channel.source === "learned" && !known.has(channel.logical_channel))
    .map((channel) => ({ lcn: channel.logical_channel, freq_hz: channel.freq_hz }));
}

export function otherControlLabel(freqs: readonly number[]): string {
  return freqs.map((hz) => formatHz(hz)).join(", ");
}

export function searchSummary(
  ranges: readonly TrunkSearchRange[] | undefined,
  candidates: number,
  searching: number,
  probes: number,
): string {
  const named = searchCandidates(ranges ?? []);
  const covering = named > 0 ? named : candidates;
  const where = named > 0 ? "the band you named" : "everything the radio can hear";
  if (covering === 0) {
    return "Covering everything the radio can hear, once the site identifies itself.";
  }
  if (searching === 0) {
    return `Covering ${where}: ${covering} frequencies.`;
  }
  return `Hunting ${searching} logical channel${searching === 1 ? "" : "s"} across ${covering} frequencies with ${probes} receiver${probes === 1 ? "" : "s"}.`;
}

export type TrunkChannelRole = "control" | "call" | "search";

export interface TrunkChannelOwner {
  node: string;
  role: TrunkChannelRole;
}

export function trunkChannelRoles(
  trunks: readonly TrunkSystemStatus[],
  deviceSet: number,
): Map<number, TrunkChannelOwner> {
  const owners = new Map<number, TrunkChannelOwner>();
  for (const system of trunks) {
    if (system.control != null && system.control.device_set === deviceSet) {
      owners.set(system.control.channel, { node: system.node, role: "control" });
    }
    for (const follower of system.followers) {
      if (follower.device_set === deviceSet) {
        owners.set(follower.channel, { node: system.node, role: "call" });
      }
    }
    for (const probe of system.probes ?? []) {
      if (probe.device_set === deviceSet) {
        owners.set(probe.channel, { node: system.node, role: "search" });
      }
    }
  }
  return owners;
}
