import { useMutation } from "@tanstack/react-query";
import { Button } from "../../components/BaseControls";
import { FACE_BTN, FACE_BTN_DANGER } from "../../components/controls";
import { ANY_FREQUENCY, inTuningRange, tuningRange } from "../../components/dial";
import { dialId, FrequencyDial } from "../../components/FrequencyDial";
import { FaceFault } from "../../components/face/Fault";
import { Readout, Readouts } from "../../components/face/Readouts";
import { FaceStats, Stat } from "../../components/face/Stats";
import { formatCount, formatMhz } from "../../components/format";
import { TuneTo } from "../../components/TuneTo";
import { calibrateArray, startArrayRecording, stopArrayRecording } from "../../lib/api";
import { failureText, isNeed, shownCenterHz, useArrayStore } from "../../lib/arrays";
import { clearAction, failAction } from "../../lib/refusals";
import type { ArrayFailure, ArrayNode, ArrayStatus, DeviceSet, PatchNode } from "../../lib/types";
import { useArrayTune } from "../../lib/useArrayTune";
import { useNow } from "../../lib/useNow";
import { useWorkspaceSelect, type Workspace } from "../context";
import { hasFeed, wiredLaneCount } from "../lanes";
import { ALIASING, ALIASING_TITLE, aliasing } from "./ArrayGeometryEditor";
import { ArrayLanes } from "./ArrayLanes";
import { ArraySettings, NO_HEADING, NO_HEADING_TITLE, useArrayEdit } from "./ArraySettings";
import {
  arrayLaneRows,
  arraySpanHz,
  arraySubtitle,
  calLabel,
  calTitle,
  failureTitle,
  headingLabel,
  laneGaps,
  memberDevices,
  recordingLabel,
  syncLabel,
  tierLabel,
} from "./arrayNode";
import { FaceBody, FaceFooter, NodeShell, useFaceActive } from "./NodeShell";

const AGE_TICK_MS = 1_000;

export const CALIBRATE_ACTION = "Calibrate";
export const RECORD_ACTION = "Rec";

function useArrayAction(node: string, action: string, run: () => Promise<unknown>) {
  return useMutation({
    mutationFn: run,
    onSuccess: () => clearAction(node, action),
    onError: (error) => failAction(node, action, error),
  });
}

export function ArrayFace({ node }: { node: PatchNode }) {
  const status = useArrayStore((store) => store.byNode[node.id]);
  const now = useNow(AGE_TICK_MS);
  const edit = useArrayEdit(node.id);
  const { lanes, rows, members, memberCount, positionWired, span } = useWorkspaceSelect(
    (workspace) => arrayView(workspace, node.id, status),
  );
  if (node.kind !== "array") {
    return null;
  }
  const failure = status?.failure ?? null;
  return (
    <NodeShell
      node={node}
      title="Array"
      category="tool"
      subtitle={arraySubtitle(status, rows.length)}
    >
      <FaceBody>
        <ArrayDial node={node.id} status={status} lead={members[0]} />
        <ArrayStatusReadout
          status={status}
          orientation={node.data.orientation}
          positionWired={positionWired}
          now={now}
        />
        <ArrayFailureLine failure={failure} lanes={rows.length} />
        <ArrayLanes node={node.id} rows={rows} status={status} />
      </FaceBody>
      <ArraySettings
        node={node.id}
        data={node.data}
        status={status}
        members={members}
        memberCount={memberCount}
        lanes={lanes}
        positionWired={positionWired}
        span={span}
        edit={edit}
      />
      <ArrayFooter
        node={node.id}
        status={status}
        aliased={aliasing(node.data.geometry, lanes, status?.center_hz ?? null)}
        lanes={rows.length}
      />
    </NodeShell>
  );
}

/** What the face shows from the workspace, as plain values, so a change elsewhere leaves it alone. */
function arrayView(workspace: Workspace, id: string, status: ArrayStatus | undefined) {
  const { resolved, devices } = workspace;
  const memberNodes = memberDevices(resolved, id);
  return {
    lanes: wiredLaneCount(resolved, id),
    rows: arrayLaneRows(workspace.graph, resolved, devices, id, status),
    members: memberNodes.flatMap((member) => {
      const set = devices.get(member);
      return set === undefined ? [] : [set];
    }),
    memberCount: memberNodes.length,
    positionWired: hasFeed(resolved, id, "position"),
    span: arraySpanHz(resolved, devices, id, status),
  };
}

function ArrayFooter({
  node,
  status,
  aliased,
  lanes,
}: {
  node: string;
  status: ArrayStatus | undefined;
  aliased: boolean;
  lanes: number;
}) {
  const calibrate = useArrayAction(node, CALIBRATE_ACTION, () => calibrateArray(node));
  const record = useArrayAction(node, RECORD_ACTION, () => startArrayRecording(node));
  const stop = useArrayAction(node, RECORD_ACTION, () => stopArrayRecording(node));
  const recording = status?.recording ?? null;
  return (
    <FaceFooter>
      <FaceStats>
        {status !== undefined && <ArrayHealth status={status} />}
        {aliased && <Stat label={ALIASING} title={ALIASING_TITLE} tone="danger" />}
      </FaceStats>
      {recording === null ? (
        <Button
          type="button"
          className={FACE_BTN}
          title="Record every lane to one collection"
          disabled={status === undefined || record.isPending}
          onClick={() => record.mutate()}
        >
          Rec
        </Button>
      ) : (
        <Button
          type="button"
          className={FACE_BTN_DANGER}
          title={recording.stem}
          disabled={stop.isPending}
          onClick={() => stop.mutate()}
        >
          Stop
        </Button>
      )}
      <Button
        type="button"
        className={FACE_BTN}
        title="Line up lane phase and gain"
        disabled={calibrate.isPending || lanes === 0}
        onClick={() => calibrate.mutate()}
      >
        Calibrate
      </Button>
    </FaceFooter>
  );
}

function ArrayHealth({ status }: { status: ArrayStatus }) {
  const gaps = laneGaps(status);
  const recording = status.recording ?? null;
  return (
    <>
      {recording !== null && (
        <Stat
          label="Rec"
          title={recording.error ?? recording.stem}
          tone={recording.dropped > 0 || recording.error != null ? "danger" : undefined}
        >
          {recordingLabel(recording, status.sample_rate)}
        </Stat>
      )}
      {gaps > 0 && (
        <Stat label="Gaps" title="Samples lost and resynced" tone="warn">
          {formatCount(gaps)}
        </Stat>
      )}
      {status.realigns > 0 && (
        <Stat label="Realigns" title="Lanes lined up again" tone="warn">
          {formatCount(status.realigns)}
        </Stat>
      )}
      {status.dropped_samples > 0 && (
        <Stat label="Drops" title="Samples dropped" tone="warn">
          {formatCount(status.dropped_samples)}
        </Stat>
      )}
      {status.events_lost > 0 && (
        <Stat label="Lost" title="Lane events lost" tone="warn">
          {formatCount(status.events_lost)}
        </Stat>
      )}
    </>
  );
}

function ArrayDial({
  node,
  status,
  lead,
}: {
  node: string;
  status: ArrayStatus | undefined;
  lead: DeviceSet | undefined;
}) {
  const { tuneArray } = useArrayTune();
  const centerHz = useArrayStore((store) => shownCenterHz(store, node));
  const active = useFaceActive();
  const range = lead === undefined ? ANY_FREQUENCY : tuningRange(lead.capabilities);
  const held = status === undefined;
  const tune = (hz: number): void => tuneArray(node, hz);
  return (
    <div className="@container flex min-w-0 items-center gap-2 p-2">
      <FrequencyDial
        id={dialId(node, 0)}
        hz={centerHz ?? 0}
        range={range}
        disabled={held}
        wheelTunes={active}
        onTune={tune}
      />
      <span className="ml-auto flex shrink-0 items-center gap-1">
        <TuneTo
          title="Type a frequency"
          hz={centerHz ?? 0}
          hint={`Reaches ${formatMhz(range.min)} to ${formatMhz(range.max)}`}
          resolve={(entered) => inTuningRange(entered, range)}
          disabled={held}
          onTune={tune}
        />
      </span>
    </div>
  );
}

function ArrayFailureLine({ failure, lanes }: { failure: ArrayFailure | null; lanes: number }) {
  if (failure === null || (failure.kind === "unwired" && lanes === 0)) {
    return null;
  }
  return isNeed(failure) ? null : (
    <FaceFault message={failureText(failure)} detail={failureTitle(failure)} />
  );
}

function ArrayStatusReadout({
  status,
  orientation,
  positionWired,
  now,
}: {
  status: ArrayStatus | undefined;
  orientation: ArrayNode["orientation"];
  positionWired: boolean;
  now: number;
}) {
  const lost = orientation.kind === "heading" && !positionWired;
  return (
    <Readouts columns={2}>
      <Readout label="Sync" title="Lanes lined up in time">
        {status === undefined ? null : syncLabel(status)}
      </Readout>
      <Readout label="Tier" title="Capped: measured drift allows less than declared">
        {status === undefined ? null : tierLabel(status)}
      </Readout>
      <Readout
        label="Cal"
        title={status === undefined ? undefined : calTitle(status)}
        tone={status?.cal === "failed" ? "danger" : undefined}
      >
        {status === undefined ? null : calLabel(status, now)}
      </Readout>
      <Readout label="Heading" title={lost ? NO_HEADING_TITLE : "Array forward, true north"}>
        {lost ? NO_HEADING : headingLabel(orientation, status)}
      </Readout>
    </Readouts>
  );
}
