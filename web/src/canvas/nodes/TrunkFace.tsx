import { Chips, ChoiceChip, ToggleChip } from "../../components/face/Chips";
import { FaceFault } from "../../components/face/Fault";
import { Readout, Readouts } from "../../components/face/Readouts";
import { TextChip } from "../../components/face/TextChip";
import { formatHz } from "../../components/format";
import { useFeeds } from "../../lib/resolved";
import type {
  ChannelDescriptor,
  PatchNode,
  TrunkSystemStatus,
  TrunkTalkgroups,
} from "../../lib/types";
import { useWorkspaceContext } from "../context";
import { patchNode } from "../graph";
import { ChannelPlanTable } from "./ChannelPlanTable";
import { FaceBody, NodeShell } from "./NodeShell";
import {
  AUTO_PROTOCOL,
  activeScheme,
  adoptable,
  awaitingControlChannel,
  channelPlanRows,
  controlChannelStalled,
  fallbackControl,
  followerCall,
  formatSearchRanges,
  formatTalkgroups,
  otherControlLabel,
  parseControlHz,
  parseSearchRanges,
  parseTalkgroups,
  planLabel,
  searchSummary,
  siteLabel,
  slotLabel,
  trunkDecoders,
  trunkProtocolLabel,
  trunkProtocols,
  trunkSpec,
} from "./trunk";

export function TrunkFace({ node }: { node: PatchNode }) {
  const workspace = useWorkspaceContext();
  const onIq = useFeeds(node.id, "iq").length > 0;
  if (node.kind !== "trunk") {
    return null;
  }
  const status = workspace.trunks.find((system) => system.node === node.id);
  const awaiting = awaitingControlChannel(onIq, node.data.control_hz);
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
        awaiting ? (
          <span className="text-warn">no control channel</span>
        ) : onIq ? (
          [
            protocol === AUTO_PROTOCOL ? trunkProtocolLabel(scheme) : null,
            `${followers.length} following`,
          ]
            .filter(Boolean)
            .join(" · ")
        ) : undefined
      }
    >
      <FaceBody>
        <Chips>
          <DecoderChip
            types={workspace.context.channelTypes}
            decoder={node.data.decoder ?? ""}
            onChange={(decoder) => edit({ decoder })}
          />
          <ChoiceChip
            label="Protocol"
            title="Trunking protocol"
            value={protocol}
            options={trunkProtocols(spec)}
            quiet={protocol === AUTO_PROTOCOL}
            onChange={(next) => edit({ protocol: next })}
          />
          <TextChip
            label="Control"
            name="Control channel"
            title={
              awaiting
                ? "Name the control channel in MHz. The radio stays untuned until then"
                : "Control channel in MHz"
            }
            value={node.data.control_hz == null ? "" : (node.data.control_hz / 1e6).toString()}
            shown={node.data.control_hz == null ? "none" : undefined}
            placeholder="451.0125"
            onCommit={(text) => edit({ control_hz: parseControlHz(text) })}
          />
          <ToggleChip
            label="Search"
            title="Find the rest of the site's channels"
            on={discovery.enabled ?? false}
            onChange={(next) => edit({ discovery: { ...discovery, enabled: next } })}
          />
          {discovery.enabled === true && (
            <TextChip
              label="Range"
              name="Search range"
              title={`Narrow the search to start-end in MHz / step in kHz. ${summary}`}
              value={formatSearchRanges(discovery.ranges)}
              placeholder="whole band"
              onCommit={(text) =>
                edit({ discovery: { ...discovery, ranges: parseSearchRanges(text) } })
              }
            />
          )}
        </Chips>
        <TalkgroupChips
          talkgroups={talkgroups}
          onChange={(next) => edit({ talkgroups: { ...talkgroups, ...next } })}
        />
        <TrunkActivity status={status} slots={slots} controlHz={node.data.control_hz} />
        {stalled && (
          <FaceFault
            message="Control channel not running"
            detail="Check it sits inside the radio's passband."
          />
        )}
        {status?.problems.map((problem) => (
          <FaceFault
            key={`${problem.freq_hz}-${problem.slot}`}
            message={`Cannot follow ${formatHz(problem.freq_hz)}${slots > 1 ? ` TS ${problem.slot}` : ""}`}
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

function TalkgroupChips({
  talkgroups,
  onChange,
}: {
  talkgroups: TrunkTalkgroups;
  onChange: (next: Partial<TrunkTalkgroups>) => void;
}) {
  const priority = talkgroups.priority;
  return (
    <Chips>
      <TextChip
        label="Priority"
        name="Priority talkgroups"
        title="Talkgroups that take a receiver from other calls when all are busy"
        value={formatTalkgroups(priority)}
        shown={priority.length === 0 ? "none" : undefined}
        placeholder="101, 102"
        onCommit={(text) => onChange({ priority: parseTalkgroups(text) })}
      />
      <TextChip
        label="Lockout"
        name="Locked out talkgroups"
        title="Talkgroups never followed"
        value={formatTalkgroups(talkgroups.lockout)}
        shown={talkgroups.lockout.length === 0 ? "none" : undefined}
        placeholder="900"
        onCommit={(text) => onChange({ lockout: parseTalkgroups(text) })}
      />
      {priority.length > 0 && (
        <ToggleChip
          label="Only these"
          title="Follow only the priority talkgroups"
          on={talkgroups.listed_only}
          onChange={(next) => onChange({ listed_only: next })}
        />
      )}
    </Chips>
  );
}

function TrunkActivity({
  status,
  slots,
  controlHz,
}: {
  status: TrunkSystemStatus | undefined;
  slots: number;
  controlHz: number | null | undefined;
}) {
  const followers = status?.followers ?? [];
  const probes = status?.probes ?? [];
  const otherControl = status?.other_control_hz ?? [];
  const site = siteLabel(status?.site);
  const fallback = fallbackControl(status, controlHz);
  if (
    followers.length === 0 &&
    probes.length === 0 &&
    otherControl.length === 0 &&
    site === null &&
    fallback === null
  ) {
    return null;
  }
  return (
    <Readouts>
      {site !== null && (
        <Readout label="Site" title="Identity the control channel broadcasts">
          {site}
        </Readout>
      )}
      {fallback !== null && (
        <Readout label="Control" title="The set control channel went quiet">
          {formatHz(fallback)}
        </Readout>
      )}
      {followers.map((follower) => (
        <Readout
          key={`${follower.freq_hz}-${follower.slot}`}
          label={slotLabel(follower.slot, slots)}
        >
          {followerCall(follower)}
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
