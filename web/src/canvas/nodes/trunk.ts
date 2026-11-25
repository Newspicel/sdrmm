import { formatHz } from "../../components/format";
import type { Form, Settings } from "../../lib/settingsForm";
import type {
  ChannelDescriptor,
  HeardTalkgroup,
  SettingField,
  TalkgroupName,
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
  TrunkTalkgroups,
  VoiceKey,
} from "../../lib/types";

export const MAX_SEARCH_RANGES = 8;
export const MAX_SEARCH_CANDIDATES = 512;
export const MIN_SEARCH_STEP_HZ = 1_250;
export const MAX_CHANNEL_MAP = 512;
export const MAX_TALKGROUPS = 256;
export const MAX_TALKGROUP_NAME_LEN = 48;
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

/** Talkgroups typed by number or by name, comma separated; unknown names drop out. */
const MAX_TALKGROUP_ID = 0xffff_ffff;

export type TalkgroupFollow = "normal" | "priority" | "lockout";

/** One talkgroup per entry: its number, the name shown on calls, and how it is followed. */
export const TALKGROUP_ITEM: Form = [
  {
    key: "id",
    label: "Talkgroup",
    control: { type: "number", min: 0, max: MAX_TALKGROUP_ID, step: 1 },
  },
  {
    key: "name",
    label: "Name",
    control: { type: "text", placeholder: "Name", optional: true, max_len: MAX_TALKGROUP_NAME_LEN },
  },
  {
    key: "follow",
    label: "Follow",
    fallback: "normal",
    control: {
      type: "choice",
      options: [
        { value: "normal", label: "Normal" },
        {
          value: "priority",
          label: "Priority",
          title: "Takes a receiver from other calls when all are busy",
        },
        { value: "lockout", label: "Locked out", title: "Never followed" },
      ],
    },
  },
];

function followOf(id: number, talkgroups: TrunkTalkgroups): TalkgroupFollow {
  if (talkgroups.lockout.includes(id)) {
    return "lockout";
  }
  return talkgroups.priority.includes(id) ? "priority" : "normal";
}

/** The talkgroups as list entries: every one named, prioritised or locked out, once. */
export function talkgroupEntries(talkgroups: TrunkTalkgroups): Settings[] {
  const ids = new Set([
    ...(talkgroups.names ?? []).map((entry) => entry.id),
    ...talkgroups.priority,
    ...talkgroups.lockout,
  ]);
  return [...ids].map((id) => ({
    id,
    name: nameOf(id, talkgroups.names) ?? "",
    follow: followOf(id, talkgroups),
  }));
}

/** The listed talkgroups, then each one heard but not listed yet, ready to name or rule. */
export function withHeard(
  entries: readonly Settings[],
  heard: readonly HeardTalkgroup[] | undefined,
): Settings[] {
  const listed = new Set(entries.map((entry) => entry.id));
  const unlisted = (heard ?? [])
    .filter((talkgroup) => !listed.has(talkgroup.id))
    .map((talkgroup) => ({ id: talkgroup.id, name: "", follow: "normal" }));
  return [...entries, ...unlisted];
}

const HEARD_SHOWN = 5;

/** The talkgroups heard most recently, each with its calls. */
export function heardLatest(
  heard: readonly HeardTalkgroup[] | undefined,
  names: readonly TalkgroupName[] | undefined,
): string {
  return (heard ?? [])
    .toSorted((a, b) => (a.last_heard < b.last_heard ? 1 : a.last_heard > b.last_heard ? -1 : 0))
    .slice(0, HEARD_SHOWN)
    .map((talkgroup) => `${talkgroupLabel(talkgroup.id, names)} ×${talkgroup.calls}`)
    .join(", ");
}

interface TalkgroupEntry {
  id: number;
  name: string;
  follow: unknown;
}

function wholeTalkgroups(entries: readonly Settings[]): TalkgroupEntry[] {
  return entries
    .flatMap(({ id, name, follow }) =>
      typeof id === "number" && Number.isSafeInteger(id) && id >= 0 && id <= MAX_TALKGROUP_ID
        ? [
            {
              id,
              follow,
              name: typeof name === "string" ? name.trim().slice(0, MAX_TALKGROUP_NAME_LEN) : "",
            },
          ]
        : [],
    )
    .slice(0, MAX_TALKGROUPS);
}

/** List entries back as names, priority and lockout, dropping any without a number yet. */
export function talkgroupsFrom(
  entries: readonly Settings[],
): Pick<TrunkTalkgroups, "names" | "priority" | "lockout"> {
  const whole = wholeTalkgroups(entries);
  const following = (follow: TalkgroupFollow): number[] => [
    ...new Set(whole.filter((entry) => entry.follow === follow).map((entry) => entry.id)),
  ];
  return {
    names: whole.filter((entry) => entry.name !== "").map(({ id, name }) => ({ id, name })),
    priority: following("priority"),
    lockout: following("lockout"),
  };
}

const CHANNEL_KIND = { key: "kind", any_of: ["channel"] };
const BAND_KIND = { key: "kind", any_of: ["band"] };

/** One row of the channel plan: a single channel, or a band that places every channel by spacing. */
export function channelItem(max: number): Form {
  return [
    {
      key: "kind",
      label: "Kind",
      fallback: "channel",
      control: {
        type: "choice",
        options: [
          { value: "channel", label: "Channel" },
          {
            value: "band",
            label: "Spaced band",
            title: "Channel N sits N spacings above channel 0",
          },
        ],
      },
    },
    {
      key: "lcn",
      label: "LCN",
      when: [CHANNEL_KIND],
      control: { type: "number", min: 0, max, step: 1 },
    },
    {
      key: "freq_hz",
      label: "Frequency",
      when: [CHANNEL_KIND],
      control: { type: "number", min: 1, step: 12_500, scale: 1e6, unit: "MHz" },
    },
    {
      key: "base_hz",
      label: "Channel 0",
      when: [BAND_KIND],
      control: { type: "number", min: 1, step: 12_500, scale: 1e6, unit: "MHz" },
    },
    {
      key: "spacing_hz",
      label: "Spacing",
      when: [BAND_KIND],
      fallback: DEFAULT_BAND_SPACING_HZ,
      control: {
        type: "number",
        min: MIN_BAND_SPACING_HZ,
        max: MAX_BAND_SPACING_HZ,
        step: 500,
        scale: 1e3,
        unit: "kHz",
      },
    },
  ];
}

/** The plan as list rows: each listed channel, then the band if there is one. */
export function channelPlanEntries(
  map: readonly TrunkChannelEntry[],
  band: TrunkBand | null | undefined,
): Settings[] {
  const channels = map.map((entry) => ({ kind: "channel", ...entry }));
  return band == null ? channels : [...channels, { kind: "band", ...band }];
}

function bandOf({ base_hz, spacing_hz }: Settings): TrunkBand | null {
  const valid =
    typeof base_hz === "number" &&
    base_hz > 0 &&
    typeof spacing_hz === "number" &&
    spacing_hz >= MIN_BAND_SPACING_HZ &&
    spacing_hz <= MAX_BAND_SPACING_HZ;
  return valid ? { base_hz: Math.round(base_hz), spacing_hz: Math.round(spacing_hz) } : null;
}

/** List rows back as the channel map and band, keeping only whole, valid rows; one band at most. */
export function channelPlanFrom(
  entries: readonly Settings[],
  max: number,
): { channel_map: TrunkChannelEntry[]; band: TrunkBand | null } {
  const band = entries
    .filter((entry) => entry.kind === "band")
    .map(bandOf)
    .find((b) => b !== null);
  const channel_map = entries.reduce<TrunkChannelEntry[]>((map, { kind, lcn, freq_hz }) => {
    const entry =
      kind !== "band" && typeof lcn === "number" && typeof freq_hz === "number"
        ? channelEntry(lcn, freq_hz / 1e6, max)
        : null;
    return entry === null ? map : withChannel(map, entry);
  }, []);
  return { channel_map, band: band ?? null };
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

/** A talkgroup by the name the operator gave it, else by number. */
export function talkgroupLabel(id: number, names: readonly TalkgroupName[] | undefined): string {
  return nameOf(id, names) ?? `TG ${id}`;
}

function nameOf(id: number, names: readonly TalkgroupName[] | undefined): string | null {
  const named = (names ?? []).findLast((entry) => entry.id === id);
  return named === undefined ? null : named.name.trim();
}

/** Who a traffic channel carries: the called talkgroup or radio, and who is talking. */
export function followerCall(
  follower: TrunkFollower,
  names: readonly TalkgroupName[] | undefined,
): string {
  const { destination, source, group_call } = follower;
  if (destination == null) {
    return "";
  }
  const called = group_call === false ? `ID ${destination}` : talkgroupLabel(destination, names);
  return source == null ? called : `${called} ← ${source}`;
}

/** Where a traffic channel sits, for its readout's title. */
export function followerPlace(follower: TrunkFollower): string {
  const parts = [formatHz(follower.freq_hz)];
  if (follower.logical_channel != null) {
    parts.push(`LCN ${follower.logical_channel}`);
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

/** A frequency typed in MHz; blank or not above zero clears it. */
export function megahertzHz(mhz: number | null): number | null {
  return mhz === null || !Number.isFinite(mhz) || mhz <= 0 ? null : Math.round(mhz * 1e6);
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

export function planSummary(rows: readonly TrunkChannel[]): string | null {
  if (rows.length === 0) {
    return null;
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
