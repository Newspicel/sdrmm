import { Button } from "../../components/BaseControls";
import { BTN_SM, TABLE_CELL, TABLE_HEAD } from "../../components/controls";
import { processorStatusOf, useArrayStore } from "../../lib/arrays";
import { STITCH_LIMITS } from "../../lib/limits";
import { isStale, readingOf, readoutsOf, useProcessorStore } from "../../lib/processors";
import type { PatchNode, StitchReading } from "../../lib/types";
import { useNow } from "../../lib/useNow";
import { useWorkspaceContext } from "../context";
import { arrayOf } from "../lanes";
import { NodeForm, StatusGrid } from "./NodeForm";
import { FaceBody, NodeShell } from "./NodeShell";
import { ProcessorFooter } from "./ProcessorFooter";
import { ProcessorError } from "./ProcessorHealth";
import { ageLabel, processorGate, processorSubtitle } from "./processorFace";
import { needsSpread, SPREAD_CHIP, spreadArrayEdit, stitchChips, stitchRow } from "./stitch";

const AGE_TICK_MS = 1_000;
const STITCH_REPORT_MS = STITCH_LIMITS.report_ms;
const LANE_HEADS = ["Lane", "MHz", "Eq", "Phase", "Coh"] as const;

export function StitchFace({ node }: { node: PatchNode }) {
  const workspace = useWorkspaceContext();
  const state = useProcessorStore((store) => store.byNode[node.id]);
  const array = arrayOf(workspace.resolved, node.id);
  const status = useArrayStore((store) => (array === null ? undefined : store.byNode[array]));
  const now = useNow(AGE_TICK_MS);
  if (node.kind !== "stitch") {
    return null;
  }
  const reading = readingOf(state, "stitch");
  const stale = state !== undefined && isStale(state.receivedAt, now, STITCH_REPORT_MS);
  const processor = processorStatusOf(status, node.id);
  const spread = needsSpread(workspace.graph, array, processorGate(status, node.id));
  const spreadArray = () => {
    workspace.edit((snapshot) => {
      const graph = spreadArrayEdit(snapshot.graph, array);
      return graph === null ? snapshot : { ...snapshot, graph };
    });
    workspace.apply();
  };
  return (
    <NodeShell
      node={node}
      title="Stitch"
      category="tool"
      subtitle={processorSubtitle(
        workspace.resolved,
        node.id,
        status,
        state,
        now,
        STITCH_REPORT_MS,
      )}
    >
      <FaceBody>
        <div className={stale ? "opacity-45" : ""}>
          <StatusGrid readouts={readoutsOf(state)} age={ageLabel(state?.receivedAt, now)} />
          <LaneTable reading={reading} />
        </div>
        <NodeForm node={node} />
        <ProcessorError status={processor} />
      </FaceBody>
      <ProcessorFooter
        status={processor}
        chips={spread ? [SPREAD_CHIP, ...stitchChips(reading)] : stitchChips(reading)}
        actions={
          spread ? (
            <Button
              type="button"
              className={BTN_SM}
              title="Tune the wired array's lanes side by side"
              onClick={spreadArray}
            >
              Spread array
            </Button>
          ) : undefined
        }
      />
    </NodeShell>
  );
}

function LaneTable({ reading }: { reading: StitchReading | null }) {
  if (reading === null || reading.lanes.length === 0) {
    return null;
  }
  return (
    <table className="w-full border-t border-line">
      <thead>
        <tr>
          {LANE_HEADS.map((head) => (
            <th key={head} className={TABLE_HEAD}>
              {head}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {reading.lanes.map((lane) => {
          const row = stitchRow(lane);
          return (
            <tr key={lane.lane} title={row.title}>
              <td className={TABLE_CELL}>{row.lane}</td>
              <td className={TABLE_CELL}>{row.mhz}</td>
              <td className={TABLE_CELL}>{row.eq}</td>
              <td className={TABLE_CELL}>{row.phase}</td>
              <td className={TABLE_CELL}>{row.coherence}</td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}
