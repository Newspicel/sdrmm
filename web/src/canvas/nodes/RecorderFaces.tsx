import { Circle } from "lucide-react";
import { Button } from "../../components/BaseControls";
import { BTN_DANGER, BTN_PRIMARY } from "../../components/controls";
import { Chips, ToggleChip } from "../../components/face/Chips";
import { FaceFault } from "../../components/face/Fault";
import { Readout, Readouts } from "../../components/face/Readouts";
import { FaceStats, Stat } from "../../components/face/Stats";
import { DROPS_HINT, formatBytes, formatCount } from "../../components/format";
import { Icon } from "../../components/Icon";
import { formatDuration, recordingElapsedS } from "../../components/recordings";
import { SAMPLE_RATE as AUDIO_RATE_HZ } from "../../lib/audio/worklet";
import { useInputs } from "../../lib/resolved";
import type { PatchNode, PatchNodeOf, RecordingStatus } from "../../lib/types";
import { useNow } from "../../lib/useNow";
import { useWorkspaceContext } from "../context";
import { patchNode } from "../graph";
import { runningSetOf } from "../workspaceDevice";
import { ownRecording, type RecorderNodeOf, withRecording, withSkipSilence } from "./audioRecorder";
import { FaceBody, FaceFooter, NodeShell } from "./NodeShell";
import { inputKey, useInputName } from "./sinkInputs";

function RecorderSwitch({
  node,
  title,
  unwired,
}: {
  node: RecorderNodeOf;
  title: string;
  unwired: string | null;
}) {
  const workspace = useWorkspaceContext();
  const recording = node.data?.recording ?? false;
  const switchTo = (next: boolean) => {
    workspace.edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (current) => withRecording(current, next)),
    }));
  };
  return (
    <Button
      type="button"
      className={recording ? BTN_DANGER : BTN_PRIMARY}
      title={recording ? undefined : (unwired ?? title)}
      disabled={unwired !== null && !recording}
      onClick={() => switchTo(!recording)}
    >
      {recording ? (
        "Stop"
      ) : (
        <>
          <Icon glyph={Circle} size={12} filled />
          Record
        </>
      )}
    </Button>
  );
}

function FileReadout({ label, file }: { label: string; file: string }) {
  return (
    <Readout label={label} title={file}>
      {file}
    </Readout>
  );
}

const IDLE = "-";

function RecordStats({
  elapsedS,
  bytes,
  samples,
  overruns,
}: {
  elapsedS?: number | null;
  bytes: number | null;
  samples?: number | null;
  overruns: number;
}) {
  return (
    <FaceStats>
      {typeof elapsedS === "number" && (
        <Stat label="Time" title="Recording length">
          {formatDuration(elapsedS)}
        </Stat>
      )}
      {bytes !== null && (
        <Stat label="Written" title="Bytes on disk">
          {formatBytes(bytes)}
        </Stat>
      )}
      {typeof samples === "number" && (
        <Stat label="Samples" title="Samples written">
          {formatCount(samples)}
        </Stat>
      )}
      {overruns > 0 && (
        <Stat label="Drops" title={DROPS_HINT} tone="warn">
          {formatCount(overruns)}
        </Stat>
      )}
    </FaceStats>
  );
}

export function RecorderFace({ node }: { node: PatchNode }) {
  if (node.kind !== "recorder") {
    return null;
  }
  return <IqRecorder node={node} />;
}

function IqRecorder({ node }: { node: PatchNodeOf<"recorder"> }) {
  const workspace = useWorkspaceContext();
  const set = runningSetOf(workspace, node.id);
  const status = set?.recording?.node === node.id ? set.recording : null;
  const recording = node.data?.recording ?? false;
  return (
    <NodeShell node={node} title="Recorder" category="output">
      <FaceBody>
        <Readouts ruled={false}>
          {status === null ? (
            <Readout label="File">{recording && set !== null ? "Waiting" : IDLE}</Readout>
          ) : (
            <FileReadout label="File" file={status.file} />
          )}
        </Readouts>
        {status?.error != null && <FaceFault message={status.error} />}
      </FaceBody>
      <FaceFooter>
        <IqRecordStats status={status} sampleRate={set?.settings.sample_rate ?? 0} />
        <RecorderSwitch
          node={node}
          title="Record IQ to a SigMF pair"
          unwired={set === null ? "Wire a device's IQ in" : null}
        />
      </FaceFooter>
    </NodeShell>
  );
}

function IqRecordStats({
  status,
  sampleRate,
}: {
  status: RecordingStatus | null;
  sampleRate: number;
}) {
  const now = useNow(1000);
  return (
    <RecordStats
      elapsedS={status === null ? null : recordingElapsedS(status, now, sampleRate)}
      bytes={status?.bytes ?? null}
      overruns={status?.overruns ?? 0}
    />
  );
}

export function AudioRecorderFace({ node }: { node: PatchNode }) {
  if (node.kind !== "audio_recorder") {
    return null;
  }
  return <AudioRecorder node={node} />;
}

function SkipSilenceChip({ node }: { node: PatchNodeOf<"audio_recorder"> }) {
  const workspace = useWorkspaceContext();
  const switchTo = (skip_silence: boolean) => {
    workspace.edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (current) =>
        withSkipSilence(current, skip_silence),
      ),
    }));
  };
  return (
    <ToggleChip
      label="Skip silence"
      title="Pause while the squelch is closed"
      on={node.data?.skip_silence ?? false}
      onChange={switchTo}
    />
  );
}

function AudioRecorder({ node }: { node: PatchNodeOf<"audio_recorder"> }) {
  const inputs = useInputs(node.id, "audio");
  const nameOf = useInputName();
  const recording = node.data?.recording ?? false;
  const takes = inputs.map((input) => ({ input, status: ownRecording(input.channel, node.id) }));
  const held = takes.flatMap(({ status }) => (status === null ? [] : [status]));
  return (
    <NodeShell node={node} title="Audio recorder" category="output">
      <FaceBody>
        <Chips>
          <SkipSilenceChip node={node} />
        </Chips>
        <Readouts ruled={false}>
          {takes.length === 0 && <Readout label="File">{IDLE}</Readout>}
          {takes.map(({ input, status }) =>
            status === null ? (
              <Readout key={inputKey(input)} label={nameOf(input)}>
                {recording ? "Waiting" : IDLE}
              </Readout>
            ) : (
              <FileReadout key={inputKey(input)} label={nameOf(input)} file={status.file} />
            ),
          )}
        </Readouts>
        {takes.map(({ input, status }) =>
          status?.error == null ? null : (
            <FaceFault key={inputKey(input)} message={`${nameOf(input)}: ${status.error}`} />
          ),
        )}
      </FaceBody>
      <FaceFooter>
        <RecordStats
          elapsedS={
            held.length === 0
              ? null
              : Math.max(...held.map((status) => status.frames)) / AUDIO_RATE_HZ
          }
          bytes={held.length === 0 ? null : sum(held.map((status) => status.bytes))}
          overruns={0}
        />
        <RecorderSwitch
          node={node}
          title="Record every wired input to its own WAV file"
          unwired={inputs.length === 0 ? "Wire a channel's audio in" : null}
        />
      </FaceFooter>
    </NodeShell>
  );
}

function sum(values: readonly number[]): number {
  return values.reduce((total, value) => total + value, 0);
}

export function BasebandRecorderFace({ node }: { node: PatchNode }) {
  if (node.kind !== "baseband_recorder") {
    return null;
  }
  return <BasebandRecorder node={node} />;
}

function BasebandRecorder({ node }: { node: PatchNodeOf<"baseband_recorder"> }) {
  const inputs = useInputs(node.id, "baseband");
  const nameOf = useInputName();
  const recording = node.data?.recording ?? false;
  const takes = inputs.map((input) => ({
    input,
    status: input.channel.baseband_recording ?? null,
  }));
  const held = takes.flatMap(({ status }) => (status === null ? [] : [status]));
  return (
    <NodeShell node={node} title="Baseband recorder" category="output">
      <FaceBody>
        <Readouts ruled={false}>
          {takes.length === 0 && <Readout label="File">{IDLE}</Readout>}
          {takes.map(({ input, status }) =>
            status === null ? (
              <Readout key={input.node} label={nameOf(input)}>
                {recording ? "Waiting" : IDLE}
              </Readout>
            ) : (
              <FileReadout key={input.node} label={nameOf(input)} file={status.file} />
            ),
          )}
        </Readouts>
        {takes.map(({ input, status }) =>
          status?.error == null ? null : (
            <FaceFault key={input.node} message={`${nameOf(input)}: ${status.error}`} />
          ),
        )}
      </FaceBody>
      <FaceFooter>
        <RecordStats
          bytes={held.length === 0 ? null : sum(held.map((status) => status.bytes))}
          samples={held.length === 0 ? null : sum(held.map((status) => status.samples))}
          overruns={sum(held.map((status) => status.overruns))}
        />
        <RecorderSwitch
          node={node}
          title="Record every wired channel's baseband to its own SigMF pair"
          unwired={inputs.length === 0 ? "Wire a channel's baseband in" : null}
        />
      </FaceFooter>
    </NodeShell>
  );
}
