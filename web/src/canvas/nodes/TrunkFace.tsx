import { Chips, ChoiceChip, OptionalNumberChip, ToggleChip } from "../../components/face/Chips";
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
  HeardTalkgroup,
  PatchNode,
  PatchNodeOf,
  SettingField,
  TalkgroupName,
  TrunkSpec,
  TrunkSystemStatus,
  TrunkTalkgroups,
  VoiceKey,
} from "../../lib/types";
import { useWorkspaceNow, useWorkspaceSelect } from "../context";
import { patchNode } from "../graph";
import { Inspect, InspectSection } from "../inspector";
import { ChannelPlanTable } from "./ChannelPlanTable";
import { FaceBody, NodeShell } from "./NodeShell";
import { useUnwired, type Want } from "./needs";
import {
  AUTO_PROTOCOL,
  activeScheme,
  adoptable,
  awaitingControlChannel,
  channelItem,
  channelPlanEntries,
  channelPlanFrom,
  channelPlanRows,
  controlChannelStalled,
  fallbackControl,
  followerCall,
  followerPlace,
  formatSearchRanges,
  heardLatest,
  maxChannel,
  megahertzHz,
  otherControlLabel,
  parseSearchRanges,
  planLabel,
  searchSummary,
  siteLabel,
  slotLabel,
  TALKGROUP_ITEM,
  talkgroupEntries,
  talkgroupsFrom,
  trunkDecoders,
  trunkKeysField,
  trunkProtocolLabel,
  trunkProtocols,
  trunkSpec,
  voiceKeys,
  withHeard,
} from "./trunk";

const WANTS_IQ: readonly Want[] = [{ port: "iq" }];

export function TrunkFace({ node }: { node: PatchNode }) {
  const now = useWorkspaceNow();
  const status = useWorkspaceSelect((workspace) =>
    workspace.trunks.find((system) => system.node === node.id),
  );
  const channelTypes = useWorkspaceSelect((workspace) => workspace.context.channelTypes);
  const onIq = useFeeds(node.id, "iq").length > 0;
  const unwired = useUnwired(node.id, WANTS_IQ);
  if (node.kind !== "trunk") {
    return null;
  }
  const awaiting = awaitingControlChannel(onIq, node.data.control_hz);
  const need = !unwired && awaiting ? "Set the control channel" : null;
  const stalled = controlChannelStalled(onIq, node.data.control_hz, status?.carriers);
  const protocol = node.data.protocol ?? AUTO_PROTOCOL;
  const detected = status?.detected ?? null;
  const spec = trunkSpec(channelTypes, node.data.decoder ?? "");
  const scheme = activeScheme(spec, protocol, detected);
  const slots = spec?.slots ?? 1;
  const talkgroups: TrunkTalkgroups = {
    priority: [],
    lockout: [],
    listed_only: false,
    names: [],
    ...node.data.talkgroups,
  };
  const keysField = trunkKeysField(channelTypes, node.data.decoder ?? "");
  const vocoder = spec?.vocoder === true ? (node.data.vocoder ?? null) : undefined;
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
    now().edit((snapshot) => ({
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
              types={channelTypes}
              data={node.data}
              protocol={protocol}
              spec={spec}
              awaiting={awaiting}
              discovery={discovery}
              searchTitle={summary}
              onEdit={edit}
            />
            {vocoder !== undefined && (
              <VocoderChip vocoder={vocoder} onChange={(next) => edit({ vocoder: next })} />
            )}
            {keysField !== null && (
              <KeysChip
                field={keysField}
                keys={node.data.keys ?? []}
                onChange={(keys) => edit({ keys })}
              />
            )}
          </Chips>
        </InspectSection>
        <InspectSection label="Calls">
          <TalkgroupChips
            talkgroups={talkgroups}
            heard={status?.heard}
            onChange={(next) => edit({ talkgroups: { ...talkgroups, ...next } })}
          />
        </InspectSection>
        <InspectSection label="Channel plan">
          <Chips inset="none">
            <ListChip
              label="Channels"
              title="Channels the site never announces: one by one, or a band spaced from channel 0"
              item={channelItem(maxChannel(spec))}
              entries={channelPlanEntries(channelMap, node.data.band)}
              quiet={channelMap.length === 0 && node.data.band == null}
              onChange={(next) => edit(channelPlanFrom(next, maxChannel(spec)))}
            />
          </Chips>
        </InspectSection>
      </Inspect>
      <FaceBody>
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
          hint={need}
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
      <OptionalNumberChip
        label="Control channel"
        title={
          awaiting
            ? "The control channel the site announces on. The radio stays untuned until it is set"
            : "The control channel the site announces on"
        }
        value={data.control_hz == null ? null : data.control_hz / 1e6}
        placeholder="none"
        unit="MHz"
        min={0}
        step={0.0125}
        onCommit={(mhz) => onEdit({ control_hz: megahertzHz(mhz) })}
      />
      <ToggleChip
        label="Find channels"
        title="Search for the site's other channels"
        on={discovery.enabled ?? false}
        onChange={(next) => onEdit({ discovery: { ...discovery, enabled: next } })}
      />
      {discovery.enabled === true && (
        <TextChip
          label="Search range"
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

function TalkgroupChips({
  talkgroups,
  heard,
  onChange,
}: {
  talkgroups: TrunkTalkgroups;
  heard: readonly HeardTalkgroup[] | undefined;
  onChange: (next: Partial<TrunkTalkgroups>) => void;
}) {
  const listed = talkgroupEntries(talkgroups);
  const entries = withHeard(listed, heard);
  return (
    <Chips inset="none">
      <ListChip
        label="Talkgroups"
        title="Talkgroups listed or heard, with the name shown on calls and how each is followed"
        item={TALKGROUP_ITEM}
        entries={entries}
        quiet={listed.length === 0}
        onChange={(next) => onChange(talkgroupsFrom(next))}
      />
      {talkgroups.priority.length > 0 && (
        <ToggleChip
          label="Priority only"
          title="Follow only the priority talkgroups"
          on={talkgroups.listed_only}
          onChange={(next) => onChange({ listed_only: next })}
        />
      )}
    </Chips>
  );
}

function KeysChip({
  field,
  keys,
  onChange,
}: {
  field: SettingField;
  keys: VoiceKey[];
  onChange: (keys: VoiceKey[]) => void;
}) {
  if (field.control.type !== "list") {
    return null;
  }
  return (
    <ListChip
      label={field.label}
      title={field.title ?? field.label}
      item={field.control.item}
      entries={keys}
      empty={field.control.empty}
      quiet={isQuiet(field, keys)}
      onChange={(entries) => onChange(voiceKeys(entries))}
    />
  );
}

function VocoderChip({
  vocoder,
  onChange,
}: {
  vocoder: string | null;
  onChange: (vocoder: string | null) => void;
}) {
  return (
    <SerialPortChip
      label="Vocoder"
      title="blip25 in software, or a DVSI AMBE-3000/3003 chip such as a ThumbDV or USB-3003"
      quiet={false}
      empty="blip25"
      value={vocoder}
      onCommit={onChange}
    />
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
    <Readouts columns="fit">
      <Readout label="Site" title="Identity the control channel broadcasts">
        {siteLabel(status?.site) ?? ""}
      </Readout>
      <Readout label="Talkgroups" title={heardLatest(status?.heard, names) || "Heard calling"}>
        {status?.heard?.length ?? 0}
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
