import { Button } from "../../components/BaseControls";
import { broadcastTitle } from "../../components/broadcastStatus";
import { ChannelControls, ChannelDial } from "../../components/ChannelControls";
import {
  channelFrequencyHz,
  channelHasAudio,
  channelWidthHz,
  radioWindowHz,
  reachesHz,
} from "../../components/channelSettings";
import { BTN_PRIMARY } from "../../components/controls";
import { ANY_FREQUENCY, tuningRange } from "../../components/dial";
import { dialId } from "../../components/FrequencyDial";
import { FaceStats, Stat } from "../../components/face/Stats";
import {
  DROPS_HINT,
  formatCount,
  formatHz,
  REJECTED_HINT,
  SKIPPED_HINT,
} from "../../components/format";
import { SignalRow } from "../../components/SignalRow";
import { useBroadcast } from "../../lib/broadcast";
import { audioPassing, heardHz, useLevelStore } from "../../lib/levels";
import { channelQueueSummary, queueLagging, usePipelineHealth } from "../../lib/pipeline";
import { controllerOf, heldByOf, runsOf, sweptBy } from "../../lib/resolved";
import type { BandMiss, BroadcastStatus, PatchNode, PatchNodeOf } from "../../lib/types";
import { channelSettingsOf, liveChannelOf, useChannelEdit } from "../../lib/useChannelEdit";
import type { ChannelEdit } from "../../lib/useChannelPatch";
import { useWorkspaceContext } from "../context";
import { nodeOf, patchNode } from "../graph";
import { iqLanesOf } from "../lanes";
import { laneOf, runningSetOf } from "../workspaceDevice";
import { bandMissOf, bandMissSaid } from "./bandMiss";
import { type Unbound, unboundOf } from "./channelNode";
import { laneCenterHz, laneRateHz } from "./deviceNode";
import { FaceBody, FaceFooter, NodeShell } from "./NodeShell";

const AUDIO_FACE_W = 460;

type ChannelNodeData = PatchNodeOf<"channel">["data"];

export function ChannelFace({ node }: { node: PatchNode }) {
  const workspace = useWorkspaceContext();
  const set = runningSetOf(workspace, node.id);
  const levels = useLevelStore((state) => (set === null ? undefined : state.byDeviceSet[set.id]));
  const editChannel = useChannelEdit();
  const live = liveChannelOf(workspace, node.id);
  const broadcast = useBroadcast(node.id);
  if (node.kind !== "channel") {
    return null;
  }

  const typeId = node.data.channel_type;
  const descriptor = workspace.context.channelTypes.find((type) => type.type_id === typeId);
  const name = descriptor?.name ?? typeId.toUpperCase();
  const channel = workspace.channels.get(node.id) ?? null;
  const lanes = iqLanesOf(workspace.resolved, node.id);
  const source = laneOf(workspace, node.id);
  const unbound = unboundOf(runsOf(workspace.resolved, node.id));
  const centerHz = set === null ? null : laneCenterHz(set, source?.stream ?? 0);
  const settings = channelSettingsOf(workspace, node.id);
  const onEdit = (edit: ChannelEdit): void => editChannel(node.id, edit);
  const level = live === null ? undefined : levels?.[live.id];
  const frequencyHz = settings === null ? null : heardHz(channelFrequencyHz(settings), level);
  const spanHz = set === null ? undefined : laneRateHz(set, source?.stream ?? 0);
  const window = radioWindowHz(centerHz, spanHz, descriptor);
  const miss = bandMissOf({
    reported: channel === null ? undefined : (channel.out_of_band ?? null),
    reaches: set === null || frequencyHz === null || reachesHz(frequencyHz, window),
  });
  const locked = node.data.tuning_locked ?? false;
  const scanned = sweptBy(workspace.resolved, node.id);
  const controller = controllerOf(workspace.resolved, node.id);
  const editNode = (next: Partial<ChannelNodeData>): void =>
    workspace.edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (current) =>
        current.kind === "channel" ? { ...current, data: { ...current.data, ...next } } : current,
      ),
    }));

  const carrier =
    lanes.length > 1 && source !== null && set !== null
      ? (nodeOf(workspace.graph, source.source)?.label ?? set.device.label)
      : null;
  const status = faceStatus({
    live: live !== null,
    unbound,
    miss,
    driver: scanned ? "scanning" : controller,
    carrier,
  });
  const widthHz = channelWidthHz(settings?.params, descriptor);
  const action = live === null ? (unbound?.action ?? null) : null;

  return (
    <NodeShell
      node={node}
      title={name}
      category="channel"
      subtitle={status}
      badge={
        widthHz === null ? undefined : <span title="Channel bandwidth">{formatHz(widthHz)}</span>
      }
      live={audioPassing(level)}
      width={channelHasAudio(descriptor) ? AUDIO_FACE_W : undefined}
    >
      <FaceBody>
        {settings !== null && (
          <div className="@container flex flex-col gap-1 border-b border-line p-2">
            <ChannelDial
              hz={heardHz(channelFrequencyHz(settings), level)}
              descriptor={descriptor}
              spanHz={spanHz ?? null}
              centerHz={centerHz}
              range={set === null ? ANY_FREQUENCY : tuningRange(set.capabilities)}
              dialId={dialId(node.id)}
              wheelTunes={workspace.selected === node.id}
              locked={locked}
              heldBy={heldByOf(workspace.resolved, node.id)}
              onTune={(frequency_hz) => onEdit({ frequency_hz })}
              onLock={(tuning_locked) => editNode({ tuning_locked })}
            />
            {live !== null && (
              <SignalRow
                level={levels?.[live.id]}
                squelch={settings.squelch}
                onSquelch={
                  channelHasAudio(descriptor) ? (squelch) => onEdit({ squelch }) : undefined
                }
              />
            )}
          </div>
        )}
        {settings !== null && (
          <ChannelControls
            settings={settings}
            descriptor={descriptor}
            broadcast={broadcast}
            onEdit={onEdit}
          />
        )}
      </FaceBody>
      <FaceFooter>
        {live !== null && (
          <ChannelHealth
            deviceSet={live.deviceSet}
            channel={live.id}
            broadcast={broadcast}
            rejected={channel?.rejected ?? 0}
            skipped={channel?.skipped ?? 0}
          />
        )}
        {action !== null && (
          <Button
            type="button"
            className={BTN_PRIMARY}
            title={unbound?.hint}
            onClick={workspace.apply}
          >
            {action}
          </Button>
        )}
      </FaceFooter>
    </NodeShell>
  );
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

function ChannelHealth({
  deviceSet,
  channel,
  broadcast,
  rejected,
  skipped,
}: {
  deviceSet: number;
  channel: number;
  broadcast: BroadcastStatus | undefined;
  rejected: number;
  skipped: number;
}) {
  const health = usePipelineHealth((state) => state.health);
  const summary = channelQueueSummary(health, deviceSet, channel);
  const dropped = summary?.dropped ?? 0;
  if (
    !queueLagging(summary) &&
    dropped === 0 &&
    broadcast === undefined &&
    rejected === 0 &&
    skipped === 0
  ) {
    return null;
  }
  return (
    <FaceStats>
      {broadcast !== undefined && <BroadcastStats status={broadcast} />}
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

function BroadcastStats({ status }: { status: BroadcastStatus }) {
  const title = broadcastTitle(status);
  if (!status.locked) {
    return <Stat label="No lock" title={title} tone="warn" />;
  }
  return (
    <Stat label="SNR" title={title} tone="ok">
      {status.snr_db.toFixed(1)} dB
    </Stat>
  );
}
