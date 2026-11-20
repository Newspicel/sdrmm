import { Chips, ChoiceChip, ToggleChip } from "../../components/face/Chips";
import { FaceFault } from "../../components/face/Fault";
import { Readout, Readouts } from "../../components/face/Readouts";
import { TextChip } from "../../components/face/TextChip";
import { formatHz } from "../../components/format";
import { ListChip } from "../../components/ListChip";
import { SerialPortChip } from "../../components/SerialPortChip";
import { useFeeds } from "../../lib/resolved";
import { isQuiet } from "../../lib/settingsForm";
import type {
  ChannelDescriptor,
  PatchNode,
  PatchNodeOf,
  SettingField,
  TalkgroupName,
  TrunkBand,
  TrunkSpec,
  TrunkSystemStatus,
  TrunkTalkgroups,
  VoiceKey,
} from "../../lib/types";
import { useWorkspaceContext } from "../context";
import { patchNode } from "../graph";
import { Inspect, InspectSection } from "../inspector";
import { ChannelPlanAdd, ChannelPlanTable } from "./ChannelPlanTable";
import { FaceBody, FaceEmpty, NodeShell } from "./NodeShell";
import { useUnwired, type Want } from "./needs";
import {
  AUTO_PROTOCOL,
  activeScheme,
  adoptable,
  awaitingControlChannel,
  channelPlanRows,
  controlChannelStalled,
  fallbackControl,
  followerCall,
  followerPlace,
  formatSearchRanges,
  formatTalkgroups,
  maxChannel,
  otherControlLabel,
  parseControlHz,
  parseSearchRanges,
  parseTalkgroups,
  planLabel,
  searchSummary,
  siteLabel,
  slotLabel,
  TALKGROUP_NAME_ITEM,
  talkgroupNames,
  trunkDecoders,
  trunkKeysField,
  trunkProtocolLabel,
  trunkProtocols,
  trunkSpec,
  voiceKeys,
  withBandBase,
  withBandSpacing,
} from "./trunk";

const WANTS_IQ: readonly Want[] = [{ port: "iq" }];

export function TrunkFace({ node }: { node: PatchNode }) {
  const workspace = useWorkspaceContext();
  const onIq = useFeeds(node.id, "iq").length > 0;
  const unwired = useUnwired(node.id, WANTS_IQ);
  if (node.kind !== "trunk") {
    return null;
  }
  const status = workspace.trunks.find((system) => system.node === node.id);
  const awaiting = awaitingControlChannel(onIq, node.data.control_hz);
  const need = !unwired && awaiting ? "Set the control channel" : null;
  const stalled = controlChannelStalled(onIq, node.data.control_hz, status?.carriers);
  const protocol = node.data.protocol ?? AUTO_PROTOCOL;
  const detected = status?.detected ?? null;
  const spec = trunkSpec(workspace.context.channelTypes, node.data.decoder ?? "");
  const scheme = activeScheme(spec, protocol, detected);
  const slots = spec?.slots ?? 1;
  const talkgroups: TrunkTalkgroups = {
    priority: [],
    lockout: [],
    listed_only: false,
    names: [],
    ...node.data.talkgroups,
  };
  const discovery = node.data.discovery ?? { enabled: false, ranges: [], max_probes: 0 };
  const channelMap = node.data.channel_map ?? [];
  const learned = status?.channel_map ?? [];
  const probes = status?.probes ?? [];
  const followers = status?.followers ?? [];
  const summary = searchSummary(
    discovery.ranges,
    status?.candidates ?? 0,
    status?.searching ?? 0,
    probes.length,
  );
  const following = new Set(
    followers
      .map((follower) => follower.logical_channel)
      .filter((lcn): lcn is number => lcn != null),
  );
  const edit = (next: Partial<typeof node.data>) => {
    workspace.edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (current) =>
        current.kind === "trunk" ? { ...current, data: { ...current.data, ...next } } : current,
      ),
    }));
  };
  return (
    <NodeShell
      node={node}
      title="Trunk system"
      category="tool"
      subtitle={
        !unwired && need === null
          ? [
              protocol === AUTO_PROTOCOL ? trunkProtocolLabel(scheme) : null,
              `${followers.filter((follower) => follower.active === true).length} active`,
            ]
              .filter(Boolean)
              .join(" · ")
          : undefined
      }
    >
      <Inspect node={node.id}>
        <InspectSection label="System">
          <Chips inset="none">
            <SystemChips
              types={workspace.context.channelTypes}
              data={node.data}
              protocol={protocol}
              spec={spec}
              awaiting={awaiting}
              discovery={discovery}
              searchTitle={summary}
              onEdit={edit}
            />
          </Chips>
        </InspectSection>
        <InspectSection label="Calls">
          <CallChips
            talkgroups={talkgroups}
            keys={node.data.keys ?? []}
            keysField={trunkKeysField(workspace.context.channelTypes, node.data.decoder ?? "")}
            vocoder={spec?.vocoder === true ? (node.data.vocoder ?? null) : undefined}
            onChange={(next) => edit({ talkgroups: { ...talkgroups, ...next } })}
            onKeys={(keys) => edit({ keys })}
            onVocoder={(vocoder) => edit({ vocoder })}
          />
        </InspectSection>
        <InspectSection label="Band">
          <BandChips band={node.data.band ?? null} onChange={(band) => edit({ band })} />
        </InspectSection>
        <InspectSection label="Add channel">
          <ChannelPlanAdd
            entries={channelMap}
            maxChannel={maxChannel(spec)}
            onChange={(channel_map) => edit({ channel_map })}
          />
        </InspectSection>
      </Inspect>
      <FaceBody>
        {need !== null && <FaceEmpty hint={need} />}
        <TrunkActivity
          status={status}
          slots={slots}
          controlHz={node.data.control_hz}
          names={talkgroups.names}
        />
        {stalled && (
          <FaceFault
            message="Control channel not running"
            detail="Check it sits inside the radio's passband."
          />
        )}
        {status?.problems.map((problem) => (
          <FaceFault
            key={`${problem.freq_hz}-${problem.slot}`}
            message={`Cannot follow ${formatHz(problem.freq_hz)}${slots > 1 && problem.slot != null ? ` TS ${problem.slot}` : ""}`}
            detail={problem.reason}
          />
        ))}
        {(status?.dropped ?? 0) > 0 && (
          <FaceFault
            message={`${status?.dropped} frames dropped`}
            detail="The trunk follower fell behind the decoders."
          />
        )}
        <ChannelPlanTable
          label={planLabel(scheme)}
          rows={channelPlanRows(learned, channelMap)}
          entries={channelMap}
          found={adoptable(learned, channelMap)}
          following={following}
          onChange={(channel_map) => edit({ channel_map })}
        />
      </FaceBody>
    </NodeShell>
  );
}

type TrunkData = PatchNodeOf<"trunk">["data"];

function SystemChips({
  types,
  data,
  protocol,
  spec,
  awaiting,
  discovery,
  searchTitle,
  onEdit,
}: {
  types: readonly ChannelDescriptor[];
  data: TrunkData;
  protocol: string;
  spec: TrunkSpec | null;
  awaiting: boolean;
  discovery: NonNullable<TrunkData["discovery"]>;
  searchTitle: string;
  onEdit: (next: Partial<TrunkData>) => void;
}) {
  return (
    <>
      <DecoderChip
        types={types}
        decoder={data.decoder ?? ""}
        onChange={(decoder) => onEdit({ decoder })}
      />
      <ChoiceChip
        label="Protocol"
        title="Trunking protocol"
        value={protocol}
        options={trunkProtocols(spec)}
        quiet={protocol === AUTO_PROTOCOL}
        onChange={(next) => onEdit({ protocol: next })}
      />
      <TextChip
        label="Control"
        name="Control channel"
        title={
          awaiting
            ? "Name the control channel in MHz. The radio stays untuned until then"
            : "Control channel in MHz"
        }
        value={data.control_hz == null ? "" : (data.control_hz / 1e6).toString()}
        shown={data.control_hz == null ? "none" : undefined}
        placeholder="451.0125"
        onCommit={(text) => onEdit({ control_hz: parseControlHz(text) })}
      />
      <ToggleChip
        label="Search"
        title="Find the rest of the site's channels"
        on={discovery.enabled ?? false}
        onChange={(next) => onEdit({ discovery: { ...discovery, enabled: next } })}
      />
      {discovery.enabled === true && (
        <TextChip
          label="Range"
          name="Search range"
          title={`Narrow the search to start-end in MHz / step in kHz. ${searchTitle}`}
          value={formatSearchRanges(discovery.ranges)}
          placeholder="whole band"
          onCommit={(text) =>
            onEdit({ discovery: { ...discovery, ranges: parseSearchRanges(text) } })
          }
        />
      )}
    </>
  );
}

function BandChips({
  band,
  onChange,
}: {
  band: TrunkBand | null;
  onChange: (band: TrunkBand | null) => void;
}) {
  return (
    <Chips inset="none">
      <TextChip
        label="Ch 0"
        name="Channel 0 frequency"
        title="Channel 0 in MHz. Channel N sits N spacings above it"
        value={band == null ? "" : (band.base_hz / 1e6).toString()}
        shown={band == null ? "none" : undefined}
        placeholder="423.0"
        onCommit={(text) => onChange(withBandBase(band, text))}
      />
      {band != null && (
        <TextChip
          label="Spacing"
          name="Channel spacing"
          title="Channel spacing in kHz"
          value={(band.spacing_hz / 1e3).toString()}
          placeholder="12.5"
          onCommit={(text) => onChange(withBandSpacing(band, text))}
        />
      )}
    </Chips>
  );
}

function CallChips({
  talkgroups,
  keys,
  keysField,
  vocoder,
  onChange,
  onKeys,
  onVocoder,
}: {
  talkgroups: TrunkTalkgroups;
  keys: VoiceKey[];
  keysField: SettingField | null;
  vocoder: string | null | undefined;
  onChange: (next: Partial<TrunkTalkgroups>) => void;
  onKeys: (keys: VoiceKey[]) => void;
  onVocoder: (vocoder: string | null) => void;
}) {
  const priority = talkgroups.priority;
  const names = talkgroups.names ?? [];
  return (
    <Chips inset="none">
      <ListChip
        label="Names"
        title="Talkgroup names, shown on calls and accepted in Priority and Lockout"
        item={TALKGROUP_NAME_ITEM}
        entries={names}
        quiet={names.length === 0}
        onChange={(entries) => onChange({ names: talkgroupNames(entries) })}
      />
      <TextChip
        label="Priority"
        name="Priority talkgroups"
        title="Talkgroups that take a receiver from other calls when all are busy, by number or name"
        value={formatTalkgroups(priority, names)}
        shown={priority.length === 0 ? "none" : undefined}
        placeholder="101, 102"
        onCommit={(text) => onChange({ priority: parseTalkgroups(text, names) })}
      />
      <TextChip
        label="Lockout"
        name="Locked out talkgroups"
        title="Talkgroups never followed, by number or name"
        value={formatTalkgroups(talkgroups.lockout, names)}
        shown={talkgroups.lockout.length === 0 ? "none" : undefined}
        placeholder="900"
        onCommit={(text) => onChange({ lockout: parseTalkgroups(text, names) })}
      />
      {priority.length > 0 && (
        <ToggleChip
          label="Only these"
          title="Follow only the priority talkgroups"
          on={talkgroups.listed_only}
          onChange={(next) => onChange({ listed_only: next })}
        />
      )}
      {keysField?.control.type === "list" && (
        <ListChip
          label={keysField.label}
          title={keysField.title ?? keysField.label}
          item={keysField.control.item}
          entries={keys}
          empty={keysField.control.empty}
          quiet={isQuiet(keysField, keys)}
          onChange={(entries) => onKeys(voiceKeys(entries))}
        />
      )}
      {vocoder !== undefined && (
        <SerialPortChip
          label="Vocoder"
          title="blip25 in software, or a DVSI AMBE-3000/3003 chip such as a ThumbDV or USB-3003"
          quiet={false}
          empty="blip25"
          value={vocoder}
          onCommit={onVocoder}
        />
      )}
    </Chips>
  );
}

function TrunkActivity({
  status,
  slots,
  controlHz,
  names,
}: {
  status: TrunkSystemStatus | undefined;
  slots: number;
  controlHz: number | null | undefined;
  names: readonly TalkgroupName[] | undefined;
}) {
  const followers = status?.followers ?? [];
  const probes = status?.probes ?? [];
  const otherControl = status?.other_control_hz ?? [];
  const fallback = fallbackControl(status, controlHz);
  return (
    <Readouts>
      <Readout label="Site" title="Identity the control channel broadcasts">
        {siteLabel(status?.site) ?? ""}
      </Readout>
      {fallback !== null && (
        <Readout label="Control" title="The set control channel went quiet">
          {formatHz(fallback)}
        </Readout>
      )}
      {followers.length === 0 && <Readout label={slotLabel(null, slots)}>{""}</Readout>}
      {followers.map((follower) => (
        <Readout
          key={`${follower.freq_hz}-${follower.slot}`}
          label={slotLabel(follower.slot, slots)}
          title={followerPlace(follower)}
          tone={follower.active === true ? "ok" : undefined}
          faint={follower.active !== true}
        >
          {followerCall(follower, names)}
        </Readout>
      ))}
      {probes.length > 0 && (
        <Readout label="Listening" title="Search receivers">
          {probes.map((probe) => formatHz(probe.freq_hz)).join(", ")}
        </Readout>
      )}
      {otherControl.length > 0 && (
        <Readout
          label="Also control"
          title="The site runs a control channel here too. Point the node at it if this one stops"
        >
          {otherControlLabel(otherControl)}
        </Readout>
      )}
    </Readouts>
  );
}

function DecoderChip({
  types,
  decoder,
  onChange,
}: {
  types: readonly ChannelDescriptor[];
  decoder: string;
  onChange: (decoder: string) => void;
}) {
  const options = trunkDecoders(types);
  if (options.length < 2) {
    return null;
  }
  return (
    <ChoiceChip
      label="Decoder"
      title="Decoder the control and traffic channels run"
      value={decoder === "" ? (options[0]?.value ?? "") : decoder}
      options={options}
      onChange={onChange}
    />
  );
}
