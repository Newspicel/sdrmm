import { useQuery } from "@tanstack/react-query";
import { Button } from "../../components/BaseControls";
import { broadcastTitle } from "../../components/broadcastStatus";
import { ChannelControls, ChannelDial } from "../../components/ChannelControls";
import {
  channelHasAudio,
  channelWidthHz,
  radioWindowHz,
  reachesHz,
} from "../../components/channelSettings";
import { BTN_PRIMARY } from "../../components/controls";
import { ANY_FREQUENCY, tuningRange } from "../../components/dial";
import { dialId } from "../../components/FrequencyDial";
import { FaceStats, Stat } from "../../components/face/Stats";
import { DROPS_HINT, formatCount, formatHz } from "../../components/format";
import { SignalRow } from "../../components/SignalRow";
import { devicesQuery } from "../../lib/api";
import { useBroadcast } from "../../lib/broadcast";
import { heardHz, useLevelStore } from "../../lib/levels";
import { channelQueueSummary, usePipelineHealth } from "../../lib/pipeline";
import { trackedBy, useSatelliteStore } from "../../lib/satellite";
import type { BroadcastStatus, PatchNode, PatchNodeOf } from "../../lib/types";
import { channelSettingsOf, liveChannelOf, useChannelEdit } from "../../lib/useChannelEdit";
import type { ChannelEdit } from "../../lib/useChannelPatch";
import { hasWire, iqLanesOf, tuningControllerOf } from "../binding";
import { useWorkspaceContext } from "../context";
import { nodeOf, patchNode } from "../graph";
import { deviceSetOf, laneOf } from "../workspaceDevice";
import {
  type ChannelBinding,
  channelBinding,
  channelBindingAction,
  channelBindingHint,
  channelBindingStatus,
  radioIsAttached,
  radioRefsOf,
} from "./channelNode";
import { laneCenterHz, laneRateHz } from "./deviceNode";
import { FaceBody, FaceFooter, NodeShell } from "./NodeShell";

const AUDIO_FACE_W = 460;

type ChannelNodeData = PatchNodeOf<"channel">["data"];

export function ChannelFace({ node }: { node: PatchNode }) {
  const workspace = useWorkspaceContext();
  const set = deviceSetOf(workspace, node.id);
  const levels = useLevelStore((state) => (set === null ? undefined : state.byDeviceSet[set.id]));
  const attached = useQuery(devicesQuery());
  const editChannel = useChannelEdit();
  const tracked = useSatelliteStore((store) => trackedBy(store.byNode, node.id));
  const live = liveChannelOf(workspace, node.id);
  const broadcast = useBroadcast(live?.deviceSet, live?.id);
  if (node.kind !== "channel") {
    return null;
  }

  const typeId = node.data.channel_type;
  const descriptor = workspace.context.channelTypes.find((type) => type.type_id === typeId);
  const name = descriptor?.name ?? typeId.toUpperCase();
  const channel = workspace.channels.get(node.id) ?? null;
  const lanes = iqLanesOf(workspace.graph, node.id, workspace.devices);
  const source = laneOf(workspace, node.id);
  const references = radioRefsOf(workspace.graph, node.id, workspace.devices);
  const binding = channelBinding({
    wired: hasWire(workspace.graph, node.id, "iq"),
    open: set !== null,
    named: references.length > 0,
    attached: radioIsAttached(references, attached.data?.devices ?? []),
  });
  const centerHz = set === null ? null : laneCenterHz(set, source?.stream ?? 0);
  const settings = channelSettingsOf(workspace, node.id);
  const onEdit = (edit: ChannelEdit): void => editChannel(node.id, edit);
  const level = live === null ? undefined : levels?.[live.id];
  const frequencyHz = settings === null ? null : heardHz(settings.frequency_hz, level);
  const spanHz = set === null ? undefined : laneRateHz(set, source?.stream ?? 0);
  const window = radioWindowHz(centerHz, spanHz, descriptor);
  const unreachable =
    set !== null &&
    frequencyHz !== null &&
    (channel?.out_of_band ?? !reachesHz(frequencyHz, window));
  const locked = node.data.tuning_locked ?? false;
  const scanned =
    channel !== null &&
    (set?.scanners?.some(
      (scanner) => scanner.error == null && scanner.settings.channel === channel.id,
    ) ??
      false);
  const controller = tuningControllerOf(workspace.graph, node.id);
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
    binding,
    unreachable,
    driver: scanned ? "scanning" : (tracked?.name ?? (tracked === null ? null : "satellite")),
    carrier,
  });
  const widthHz = channelWidthHz(settings?.params, descriptor);
  const action = live === null ? channelBindingAction(binding) : null;

  return (
    <NodeShell
      node={node}
      title={name}
      category="channel"
      subtitle={status}
      badge={
        widthHz === null ? undefined : <span title="Channel bandwidth">{formatHz(widthHz)}</span>
      }
      width={channelHasAudio(descriptor) ? AUDIO_FACE_W : undefined}
    >
      <FaceBody>
        {settings !== null && (
          <div className="@container flex flex-col gap-1 border-b border-line p-2">
            <ChannelDial
              hz={heardHz(settings.frequency_hz, level)}
              descriptor={descriptor}
              spanHz={spanHz ?? null}
              centerHz={centerHz}
              range={set === null ? ANY_FREQUENCY : tuningRange(set.capabilities)}
              dialId={dialId(node.id)}
              wheelTunes={workspace.selected === node.id}
              locked={locked}
              heldBy={controller === null ? null : (tracked?.name ?? controller)}
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
            broadcast={broadcast?.status}
            onEdit={onEdit}
          />
        )}
      </FaceBody>
      <FaceFooter>
        {live !== null && (
          <ChannelHealth
            deviceSet={live.deviceSet}
            channel={live.id}
            broadcast={broadcast?.status}
          />
        )}
        {action !== null && (
          <Button
            type="button"
            className={BTN_PRIMARY}
            title={channelBindingHint(binding)}
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
  binding,
  unreachable,
  driver,
  carrier,
}: {
  live: boolean;
  binding: ChannelBinding;
  unreachable: boolean;
  driver: string | null;
  carrier: string | null;
}) {
  if (!live) {
    return binding === "unwired" ? undefined : (
      <span title={channelBindingHint(binding)}>{channelBindingStatus(binding)}</span>
    );
  }
  if (driver !== null) {
    return <span title="Tuned by the node on its control input">{driver}</span>;
  }
  if (unreachable) {
    return <span className="text-warn">out of band</span>;
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
}: {
  deviceSet: number;
  channel: number;
  broadcast: BroadcastStatus | undefined;
}) {
  const health = usePipelineHealth((state) => state.health);
  const summary = channelQueueSummary(health, deviceSet, channel);
  if (summary === null && broadcast === undefined) {
    return null;
  }
  return (
    <FaceStats>
      {broadcast !== undefined && <BroadcastStats status={broadcast} />}
      {summary !== null && (
        <Stat label="Queue" title={summary.detail}>
          {summary.oldestMs.toFixed(0)} ms
        </Stat>
      )}
      {summary !== null && summary.dropped > 0 && (
        <Stat label="Drops" title={DROPS_HINT} tone="warn">
          {formatCount(summary.dropped)}
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
