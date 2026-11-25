import { Button } from "../../components/BaseControls";
import { ConfirmButton } from "../../components/ConfirmButton";
import { FACE_BTN, FACE_BTN_DANGER } from "../../components/controls";
import { ChipField, Chips, NumberChip, SettingChip } from "../../components/face/Chips";
import { Readout, Readouts } from "../../components/face/Readouts";
import { FaceStats, Stat } from "../../components/face/Stats";
import { formatHz, formatSignedHz } from "../../components/format";
import { MapPanel } from "../../components/LazyMapPanel";
import { MapSettings } from "../../components/map/MapSettings";
import { OffsetStepper } from "../../components/OffsetStepper";
import { usePositionStore } from "../../lib/position";
import { useFeeds } from "../../lib/resolved";
import {
  canStart,
  retunedSince,
  type SurveyView,
  signalOffsetLimitHz,
  signalSurveyCsv,
  surveyFrequencyHz,
  surveyStatus,
} from "../../lib/signalSurvey";
import { EMPTY_SURVEY, useSurveyControl, useSurveySeed, useSurveyStore } from "../../lib/survey";
import type { PatchNode, PatchNodeOf, SurveyCell } from "../../lib/types";
import { useWorkspaceNow, useWorkspaceSelect, type Workspace } from "../context";
import { patchNode } from "../graph";
import { Inspect, InspectSection } from "../inspector";
import { iqSourceOf } from "../lanes";
import { runningSetOf } from "../workspaceDevice";
import { laneCenterHz, laneRateHz } from "./deviceNode";
import { FaceBody, FaceFooter, NodeShell, useFaceActive } from "./NodeShell";
import { useUnwired, type Want } from "./needs";

const NO_POSITIONS: readonly string[] = [];
const MAX_OFFSET_HZ = 1_000_000_000_000;
const MAX_BANDWIDTH_HZ = 100_000_000;
const WANTS: readonly Want[] = [{ port: "iq" }, { port: "position" }];

export function SignalMapFace({ node }: { node: PatchNode }) {
  const lane = useWorkspaceSelect((workspace) => laneOfSurvey(workspace, node.id));
  const radioFeeds = useFeeds(node.id, "iq");
  const positionNode = useFeeds(node.id, "position")[0]?.source ?? null;
  const positioned = usePositionStore(
    (store) => positionNode !== null && (store.sources[positionNode]?.fix ?? null) !== null,
  );

  if (node.kind !== "signal_map") {
    return null;
  }

  return (
    <SignalSurvey
      node={node}
      centerHz={lane.centerHz}
      spanHz={lane.spanHz}
      radioWired={radioFeeds.length > 0}
      positionNode={positionNode}
      positioned={positioned}
    />
  );
}

/** The centre and span of the radio lane the survey hears, null while none runs. */
function laneOfSurvey(workspace: Workspace, node: string) {
  const set = runningSetOf(workspace, node);
  const iq = iqSourceOf(workspace.resolved, node);
  return {
    centerHz: set === null || iq === null ? null : laneCenterHz(set, iq.stream),
    spanHz: set === null || iq === null ? null : (laneRateHz(set, iq.stream) ?? null),
  };
}

export function editSurveyBand(
  workspace: Pick<Workspace, "edit" | "apply">,
  node: string,
  offsetHz: number,
  bandwidthHz: number,
): void {
  workspace.edit((snapshot) => ({
    ...snapshot,
    graph: patchNode(snapshot.graph, node, (current) =>
      current.kind === "signal_map"
        ? { ...current, data: { ...current.data, offset_hz: offsetHz, bandwidth_hz: bandwidthHz } }
        : current,
    ),
  }));
  workspace.apply();
}

function offsetLimitHz(spanHz: number | null, bandwidthHz: number): number {
  return spanHz === null
    ? MAX_OFFSET_HZ
    : Math.min(MAX_OFFSET_HZ, signalOffsetLimitHz(spanHz, bandwidthHz));
}

function SignalSurvey({
  node,
  centerHz,
  spanHz,
  radioWired,
  positionNode,
  positioned,
}: {
  node: PatchNodeOf<"signal_map">;
  centerHz: number | null;
  spanHz: number | null;
  radioWired: boolean;
  positionNode: string | null;
  positioned: boolean;
}) {
  const now = useWorkspaceNow();
  const active = useFaceActive();
  useSurveySeed(node.id);
  const survey = useSurveyStore((store) => store.byNode[node.id] ?? EMPTY_SURVEY);
  const { control, pending } = useSurveyControl(node.id);
  const unwired = useUnwired(node.id, WANTS);
  const cells = survey.cells;
  const held = cells.length > 0;
  const targetHz = radioWired ? survey.targetHz : null;
  const levelDbfs = radioWired ? survey.levelDbfs : null;
  const view: SurveyView = {
    radioWired,
    positionWired: positionNode !== null,
    positioned,
    targetHz,
    levelDbfs,
    recording: survey.recording,
    retuned: retunedSince(cells, targetHz),
  };

  const updateSettings = (offsetHz: number, bandwidthHz: number): void =>
    editSurveyBand(now(), node.id, offsetHz, bandwidthHz);

  return (
    <NodeShell
      node={node}
      title="Signal survey"
      category="output"
      subtitle={
        !unwired ? (
          <span className={survey.recording ? "text-accent" : undefined}>{surveyStatus(view)}</span>
        ) : undefined
      }
    >
      <Inspect node={node.id}>
        <InspectSection label="Band">
          <SurveyBand
            node={node}
            centerHz={centerHz}
            spanHz={spanHz}
            held={held}
            onBand={updateSettings}
          />
        </InspectSection>
        <MapSettings />
      </Inspect>
      <FaceBody scroll={false}>
        <div className="relative flex min-h-0 w-full flex-1 flex-col">
          <MapPanel
            positionNodes={positionNode === null ? NO_POSITIONS : [positionNode]}
            signalSamples={cells}
            active={active}
            className="min-h-0 w-full flex-1"
          />
        </div>
        <Readouts columns={3}>
          <Readout label="Cells">{cells.length}</Readout>
          <Readout label="At">{targetHz === null ? "- Hz" : formatHz(targetHz)}</Readout>
          <Readout
            label="Level"
            title="Relative receiver level. Keep gain and antenna fixed when comparing locations"
          >
            {levelDbfs === null ? "- dBFS" : `${levelDbfs.toFixed(1)} dBFS`}
          </Readout>
        </Readouts>
      </FaceBody>
      <FaceFooter>
        {survey.dropped > 0 && (
          <FaceStats>
            <Stat label="Dropped" title="Oldest cells dropped to stay within the limit" tone="warn">
              {survey.dropped}
            </Stat>
          </FaceStats>
        )}
        <Button
          type="button"
          className={FACE_BTN}
          disabled={!held}
          onClick={() => downloadSurvey(node, cells)}
        >
          Export CSV
        </Button>
        <ConfirmButton
          label="Clear"
          disabled={pending || !held}
          onConfirm={() => control("clear")}
        />
        <Button
          type="button"
          className={survey.recording ? FACE_BTN_DANGER : FACE_BTN}
          disabled={pending || (!survey.recording && !canStart(view))}
          aria-pressed={survey.recording}
          onClick={() => control(survey.recording ? "stop" : "start")}
        >
          {survey.recording ? "Pause" : "Start survey"}
        </Button>
      </FaceFooter>
    </NodeShell>
  );
}

function SurveyBand({
  node,
  centerHz,
  spanHz,
  held,
  onBand,
}: {
  node: PatchNodeOf<"signal_map">;
  centerHz: number | null;
  spanHz: number | null;
  held: boolean;
  onBand: (offsetHz: number, bandwidthHz: number) => void;
}) {
  return (
    <Chips inset="none">
      <SettingChip
        label="Offset"
        value={formatSignedHz(node.data.offset_hz)}
        quiet={node.data.offset_hz === 0}
        disabled={held}
        title={held ? "Clear the survey to change the offset" : "Offset from the radio centre"}
        width="w-80"
      >
        {() => (
          <ChipField label="Offset from the radio centre">
            <OffsetStepper
              offsetHz={node.data.offset_hz}
              limitHz={offsetLimitHz(spanHz, node.data.bandwidth_hz)}
              centerHz={centerHz}
              onOffset={(offset) => onBand(offset, node.data.bandwidth_hz)}
            />
          </ChipField>
        )}
      </SettingChip>
      <NumberChip
        label="Width"
        title={held ? "Clear the survey to change the width" : "Survey bandwidth"}
        value={node.data.bandwidth_hz / 1e3}
        unit="kHz"
        min={0.001}
        max={MAX_BANDWIDTH_HZ / 1e3}
        disabled={held}
        onCommit={(khz) => {
          const bandwidth = Math.round(khz * 1e3);
          if (bandwidth < 1 || bandwidth > MAX_BANDWIDTH_HZ) {
            return;
          }
          if (bandwidth !== node.data.bandwidth_hz) {
            const limit = offsetLimitHz(spanHz, bandwidth);
            onBand(Math.max(-limit, Math.min(limit, node.data.offset_hz)), bandwidth);
          }
        }}
      />
    </Chips>
  );
}

function downloadSurvey(node: PatchNodeOf<"signal_map">, cells: readonly SurveyCell[]): void {
  const blob = new Blob([signalSurveyCsv(cells, node.data.offset_hz, node.data.bandwidth_hz)], {
    type: "text/csv;charset=utf-8",
  });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  const frequency = surveyFrequencyHz(cells) ?? 0;
  link.download = `signal-survey-${frequency}-hz-${node.data.bandwidth_hz}-hz-wide.csv`;
  link.click();
  URL.revokeObjectURL(url);
}
