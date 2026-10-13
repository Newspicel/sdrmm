import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Button, Input } from "../../components/BaseControls";
import { BTN, BTN_PRIMARY, BTN_QUIET_DANGER, FIELD } from "../../components/controls";
import { FaceFault } from "../../components/face/Fault";
import { Readout, Readouts } from "../../components/face/Readouts";
import { formatBytes, formatMhz, formatSampleRate } from "../../components/format";
import { PlaybackTransport } from "../../components/PlaybackTransport";
import { RecordingUpload } from "../../components/RecordingUpload";
import {
  describeRecording,
  formatDuration,
  recordingLanes,
  recordingProvenance,
  recordingTitle,
} from "../../components/recordings";
import { recordingsQuery } from "../../lib/api";
import type { PatchNode, PatchNodeOf, RecordingInfo } from "../../lib/types";
import { useWorkspaceContext } from "../context";
import { patchNode } from "../graph";
import { MakeArrayButton, offersMakeArray } from "./MakeArrayButton";
import { FaceBody, FaceFooter, NodeShell } from "./NodeShell";
import { claimedRecordings, recordingChoices } from "./recordingNode";
import { SourceHealth } from "./SourceHealth";

type RecordingNodeData = PatchNodeOf<"recording">["data"];

function Library({ node, onPick }: { node: string; onPick: (recording: RecordingInfo) => void }) {
  const workspace = useWorkspaceContext();
  const library = useQuery(recordingsQuery());
  const [search, setSearch] = useState("");
  const all = library.data?.recordings ?? [];
  const choices = recordingChoices(all, claimedRecordings(workspace.graph, node), search);

  return (
    <div className="flex flex-col gap-2 p-2">
      <div className="flex items-center gap-2">
        {all.length > 0 && (
          <Input
            className={`${FIELD} min-w-0 flex-1`}
            type="search"
            name="recording-filter"
            placeholder="Search recordings"
            aria-label="Search recordings"
            value={search}
            onChange={(event) => setSearch(event.target.value)}
          />
        )}
        <RecordingUpload compact={all.length > 0} onUploaded={onPick} />
      </div>
      <div className="flex max-h-64 flex-col gap-1 overflow-y-auto">
        {choices.map((recording) => (
          <Button
            key={recording.id}
            type="button"
            className={`${BTN} h-auto min-h-7 shrink-0 justify-start py-1.5 text-left`}
            title={recordingProvenance(recording)}
            onClick={() => onPick(recording)}
          >
            <span className="flex w-full min-w-0 flex-col gap-0.5">
              <span className="truncate">{recordingTitle(recording)}</span>
              <span className="truncate font-mono text-3xs text-ink-dim tabular-nums">
                {describeRecording(recording)}
              </span>
            </span>
          </Button>
        ))}
      </div>
      {library.isPending && <p className="text-sm text-ink-dim">Reading the library…</p>}
      {!library.isPending && all.length === 0 && (
        <p className="text-sm text-ink-dim">No recordings yet.</p>
      )}
      {all.length > 0 && choices.length === 0 && (
        <p className="text-sm text-ink-dim">Nothing free matches that.</p>
      )}
    </div>
  );
}

export function RecordingFacts({ recording }: { recording: RecordingInfo }) {
  const lanes = recordingLanes(recording);
  return (
    <Readouts columns={2}>
      {lanes > 1 && (
        <Readout label="Lanes" title="Lanes recorded side by side">
          {lanes}
        </Readout>
      )}
      <Readout label="Centre" title="Recorded centre">
        {formatMhz(recording.center_hz)}
      </Readout>
      <Readout label="Rate" title="Recorded sample rate">
        {formatSampleRate(recording.sample_rate)}
      </Readout>
      <Readout label="Length">{formatDuration(recording.duration_s)}</Readout>
      <Readout label="Size" title="Size on disk">
        {formatBytes(recording.bytes)}
      </Readout>
    </Readouts>
  );
}

export function RecordingFace({ node }: { node: PatchNode }) {
  const workspace = useWorkspaceContext();
  const library = useQuery(recordingsQuery());
  const stem = node.kind === "recording" ? (node.data.recording ?? null) : null;
  const set = workspace.devices.get(node.id) ?? null;
  const known =
    stem === null
      ? null
      : (library.data?.recordings.find((recording) => recording.file === stem) ?? null);

  const editNode = (next: Partial<RecordingNodeData>): void =>
    workspace.edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (stored) =>
        stored.kind === "recording" ? { ...stored, data: { ...stored.data, ...next } } : stored,
      ),
    }));

  const forget = (): void => editNode({ recording: null });

  const pick = (recording: RecordingInfo): void => {
    editNode({ recording: recording.file });
    workspace.apply();
  };

  if (stem === null) {
    return (
      <NodeShell node={node} title="Recording" category="source">
        <FaceBody>
          <Library node={node.id} onPick={pick} />
        </FaceBody>
      </NodeShell>
    );
  }

  if (set === null) {
    const gone = library.isSuccess && known === null;
    return (
      <NodeShell
        node={node}
        title="Recording"
        category="source"
        subtitle={gone ? "missing" : undefined}
      >
        <FaceBody>
          <p className="p-3 font-mono text-sm text-ink">{known?.file ?? stem}</p>
          {known !== null && (
            <p className="px-3 pb-3 font-mono text-3xs text-ink-dim tabular-nums">
              {describeRecording(known)}
            </p>
          )}
        </FaceBody>
        <FaceFooter>
          <Button
            type="button"
            className={BTN_QUIET_DANGER}
            title="Free this node so you can pick another recording"
            onClick={forget}
          >
            Forget recording
          </Button>
          <Button
            type="button"
            className={BTN_PRIMARY}
            title={
              gone
                ? "This recording is no longer in the library"
                : "Start playing this recording into whatever is wired to it"
            }
            onClick={() => workspace.apply()}
            disabled={gone}
          >
            Play
          </Button>
        </FaceFooter>
      </NodeShell>
    );
  }

  return (
    <NodeShell
      node={node}
      title={known === null ? stem : recordingTitle(known)}
      category="source"
      subtitle={set.status === "error" ? <span className="text-danger">error</span> : undefined}
    >
      <FaceBody>
        {set.playback != null && (
          <PlaybackTransport node={node.id} set={set} status={set.playback} />
        )}
        {known !== null && <RecordingFacts recording={known} />}
        {set.error != null && <FaceFault message={set.error} />}
      </FaceBody>
      <FaceFooter>
        <SourceHealth set={set} />
        {offersMakeArray(workspace.resolved, node.id, set) && (
          <MakeArrayButton node={node.id} set={set} />
        )}
        <Button
          type="button"
          className={BTN_QUIET_DANGER}
          title="Stop playing and free the node: the wires stay drawn"
          onClick={forget}
        >
          Forget recording
        </Button>
      </FaceFooter>
    </NodeShell>
  );
}
