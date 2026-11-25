import { Button } from "../../components/BaseControls";
import { ChannelControls, ChannelDial, SquelchMargin } from "../../components/ChannelControls";
import {
  bandWidthHz,
  channelFrequencyHz,
  channelHasAudio,
  radioWindowHz,
  reachesHz,
} from "../../components/channelSettings";
import { FACE_BTN } from "../../components/controls";
import { ANY_FREQUENCY, tuningRange } from "../../components/dial";
import { dialId } from "../../components/FrequencyDial";
import { ChipRows } from "../../components/face/Chips";
import { HealthStats, hasHealth } from "../../components/face/Health";
import { FaceStats, Stat } from "../../components/face/Stats";
import {
  DROPS_HINT,
  formatCount,
  formatHz,
  REJECTED_HINT,
  SKIPPED_HINT,
} from "../../components/format";
import { SignalRow } from "../../components/SignalRow";
import { useChannelBand } from "../../lib/channelBand";
import { statusChoices, useChannelStatus } from "../../lib/channelStatus";
import { useChannelLevel, useChannelOpen, useChannelShiftHz } from "../../lib/levels";
import { channelQueueSummary, queueLagging, usePipelineHealth } from "../../lib/pipeline";
import { controllerOf, heldByOf, runsOf, sweptBy } from "../../lib/resolved";
import { namedReadouts } from "../../lib/settingsForm";
import type {
  BandMiss,
  ChannelStatus,
  Field,
  PatchNode,
  PatchNodeOf,
  Squelch,
} from "../../lib/types";
import {
  channelSettingsOf,
  type LiveChannel,
  liveChannelOf,
  useChannelEdit,
} from "../../lib/useChannelEdit";
import type { ChannelEdit } from "../../lib/useChannelPatch";
import { keyedFields } from "../../sinks/Fields";
import { formatValue } from "../../sinks/values";
import { useWorkspaceNow, useWorkspaceSelect, type Workspace } from "../context";
import { nodeOf, patchNode } from "../graph";
import { Inspect, InspectSection } from "../inspector";
import { iqLanesOf } from "../lanes";
import { laneOf, runningSetOf } from "../workspaceDevice";
import { bandMissOf, bandMissSaid } from "./bandMiss";
import { type Unbound, unboundOf } from "./channelNode";
import { laneCenterHz, laneRateHz } from "./deviceNode";
import { StatusGrid } from "./NodeForm";
import { FaceBody, FaceEmpty, FaceFooter, NodeShell } from "./NodeShell";

type ChannelNodeData = PatchNodeOf<"channel">["data"];

export function ChannelFace({ node }: { node: PatchNode }) {
  const now = useWorkspaceNow();
  const typeId = node.kind === "channel" ? node.data.channel_type : null;
  const view = useWorkspaceSelect((workspace) => channelView(workspace, node.id, typeId));
  const editChannel = useChannelEdit();
  const channelStatus = useChannelStatus(node.id);
  const band = useChannelBand(view.settings, view.channel);
  const shiftHz = useChannelShiftHz(view.live);
  const passing = useChannelOpen(view.live);
  if (node.kind !== "channel") {
    return null;
  }

  const { live, channel, settings, descriptor, radio, unbound } = view;
  const name = descriptor?.name ?? node.data.channel_type.toUpperCase();
  const centerHz = radio?.centerHz ?? null;
  const onEdit = (edit: ChannelEdit): void => editChannel(node.id, edit);
  const frequencyHz = settings === null ? null : channelFrequencyHz(settings) + shiftHz;
  const spanHz = radio?.spanHz;
  const window = radioWindowHz(centerHz, spanHz, band);
  const miss = bandMissOf({
    reported: channel === null ? undefined : (channel.out_of_band ?? null),
    reaches: radio === null || frequencyHz === null || reachesHz(frequencyHz, window),
  });
  const locked = node.data.tuning_locked ?? false;
  const editNode = (next: Partial<ChannelNodeData>): void =>
    now().edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (current) =>
        current.kind === "channel" ? { ...current, data: { ...current.data, ...next } } : current,
      ),
    }));

  const status = faceStatus({
    live: live !== null,
    unbound,
    miss,
    driver: view.scanned ? "scanning" : view.controller,
    carrier: view.carrier,
  });
  const widthHz = bandWidthHz(band);
  const action = live === null ? (unbound?.action ?? null) : null;
  const standing = unbound?.standing === true ? unbound.hint : null;

  return (
    <NodeShell
      node={node}
      title={name}
      category="channel"
      subtitle={status ?? "Decoder"}
      badge={
        widthHz === null ? undefined : <span title="Channel bandwidth">{formatHz(widthHz)}</span>
      }
      live={passing}
    >
      <FaceBody>
        {settings !== null && (
          <div className="@container flex flex-col gap-1 p-2">
            <ChannelDial
              hz={channelFrequencyHz(settings) + shiftHz}
              band={band}
              spanHz={spanHz ?? null}
              centerHz={centerHz}
              range={radio?.range ?? ANY_FREQUENCY}
              dialId={dialId(node.id)}
              wheelTunes={view.selected}
              locked={locked}
              heldBy={view.heldBy}
              onTune={(frequency_hz) => onEdit({ frequency_hz })}
              onLock={(tuning_locked) => editNode({ tuning_locked })}
            />
            <ChannelSignal live={live} />
          </div>
        )}
        {standing !== null && <FaceEmpty hint={standing} />}
      </FaceBody>
      <Inspect node={node.id}>
        {settings !== null && (
          <InspectSection>
            {channelHasAudio(descriptor) && (
              <div className="flex flex-col gap-2">
                <ChannelSignal
                  live={live}
                  squelch={settings.squelch}
                  onSquelch={(squelch) => onEdit({ squelch })}
                />
                <ChipRows>
                  <SquelchMargin settings={settings} onEdit={onEdit} />
                </ChipRows>
              </div>
            )}
            <ChannelControls
              settings={settings}
              descriptor={descriptor}
              band={band}
              status={channelStatus}
              onEdit={onEdit}
            />
          </InspectSection>
        )}
        {(channelStatus?.readouts ?? []).length > 0 && (
          <InspectSection label="Status">
            <StatusGrid readouts={channelStatus?.readouts ?? []} />
          </InspectSection>
        )}
      </Inspect>
      <FaceFooter>
        {live !== null && (
          <ChannelFooterStats
            deviceSet={live.deviceSet}
            channel={live.id}
            status={channelStatus}
            named={namedReadouts(descriptor?.settings ?? [], statusChoices(channelStatus))}
            rejected={channel?.rejected ?? 0}
            skipped={channel?.skipped ?? 0}
          />
        )}
        {action !== null && (
          <Button
            type="button"
            className={FACE_BTN}
            title={unbound?.hint}
            onClick={() => now().apply()}
          >
            {action}
          </Button>
        )}
      </FaceFooter>
    </NodeShell>
  );
}

/** Everything the face shows from the workspace, picked as plain values so a change elsewhere
 * leaves the face alone. */
function channelView(workspace: Workspace, id: string, typeId: string | null) {
  const set = runningSetOf(workspace, id);
  const source = laneOf(workspace, id);
  const stream = source?.stream ?? 0;
  const lanes = iqLanesOf(workspace.resolved, id);
  return {
    live: liveChannelOf(workspace, id),
    channel: workspace.channels.get(id) ?? null,
    settings: channelSettingsOf(workspace, id),
    descriptor: workspace.context.channelTypes.find((type) => type.type_id === typeId),
    radio:
      set === null
        ? null
        : {
            centerHz: laneCenterHz(set, stream),
            spanHz: laneRateHz(set, stream),
            range: tuningRange(set.capabilities),
          },
    unbound: unboundOf(runsOf(workspace.resolved, id)),
    carrier:
      lanes.length > 1 && source !== null && set !== null
        ? (nodeOf(workspace.graph, source.source)?.label ?? set.device.label)
        : null,
    scanned: sweptBy(workspace.resolved, id),
    controller: controllerOf(workspace.resolved, id),
    heldBy: heldByOf(workspace.resolved, id),
    selected: workspace.selected === id,
  };
}

/** The meter reads its own level, so the face does not redraw ten times a second. */
function ChannelSignal({
  live,
  squelch,
  onSquelch,
}: {
  live: LiveChannel | null;
  squelch?: Squelch;
  onSquelch?: (squelch: Squelch) => void;
}) {
  const level = useChannelLevel(live);
  return <SignalRow level={level} squelch={squelch} onSquelch={onSquelch} />;
}

function faceStatus({
  live,
  unbound,
  miss,
  driver,
  carrier,
}: {
  live: boolean;
  unbound: Unbound | null;
  miss: BandMiss | null;
  driver: string | null;
  carrier: string | null;
}) {
  if (!live) {
    return unbound === null || unbound.status === null ? undefined : (
      <span title={unbound.hint}>{unbound.status}</span>
    );
  }
  if (driver !== null) {
    return <span title="Tuned by the node on its control input">{driver}</span>;
  }
  if (miss !== null) {
    const said = bandMissSaid(miss);
    return (
      <span className="text-warn" title={said.title}>
        {said.label}
      </span>
    );
  }
  if (carrier !== null) {
    return <span title="The radio carrying this decoder now">{carrier}</span>;
  }
  return undefined;
}

function ChannelFooterStats({
  deviceSet,
  channel,
  status,
  named,
  rejected,
  skipped,
}: {
  deviceSet: number;
  channel: number;
  status: ChannelStatus | undefined;
  named: ReadonlySet<string>;
  rejected: number;
  skipped: number;
}) {
  const pipeline = usePipelineHealth((state) => state.health);
  const summary = channelQueueSummary(pipeline, deviceSet, channel);
  const dropped = summary?.dropped ?? 0;
  const readouts = (status?.readouts ?? []).filter((field) => !named.has(field.key));
  if (
    !queueLagging(summary) &&
    dropped === 0 &&
    !hasHealth(status?.health) &&
    readouts.length === 0 &&
    rejected === 0 &&
    skipped === 0
  ) {
    return null;
  }
  return (
    <FaceStats>
      <HealthStats health={status?.health} />
      <StatusReadouts readouts={readouts} />
      {queueLagging(summary) && (
        <Stat label="Queue" title={summary.detail} tone="warn">
          {summary.oldestMs.toFixed(0)} ms
        </Stat>
      )}
      {dropped > 0 && (
        <Stat label="Drops" title={DROPS_HINT} tone="warn">
          {formatCount(dropped)}
        </Stat>
      )}
      {rejected > 0 && (
        <Stat label="Bad" title={REJECTED_HINT} tone="warn">
          {formatCount(rejected)}
        </Stat>
      )}
      {skipped > 0 && (
        <Stat label="Skipped" title={SKIPPED_HINT} tone="warn">
          {formatCount(skipped)}
        </Stat>
      )}
    </FaceStats>
  );
}

export function StatusReadouts({ readouts }: { readouts: readonly Field[] }) {
  return keyedFields(readouts).map(([key, field]) => (
    <Stat key={key} label={field.label} title={field.label}>
      {formatValue(field.value)}
    </Stat>
  ));
}
