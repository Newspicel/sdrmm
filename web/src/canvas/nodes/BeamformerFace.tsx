import { MeterBar } from "../../components/face/Meter";
import { processorStatusOf, useArrayStore } from "../../lib/arrays";
import { isStale, readingOf, readoutsOf, useProcessorStore } from "../../lib/processors";
import type { BeamformerReading, PatchNode } from "../../lib/types";
import { useNow } from "../../lib/useNow";
import { useWorkspaceContext } from "../context";
import { arrayOf } from "../lanes";
import {
  amplitudePercent,
  angleGap,
  beamChips,
  NULL_CLOSE_DEG,
  patternPath,
  polarTick,
  weightTitle,
} from "./beamformer";
import { NodeForm, nodeSettings, StatusGrid } from "./NodeForm";
import { FaceBody, NodeShell } from "./NodeShell";
import { ProcessorFooter } from "./ProcessorFooter";
import { ProcessorError } from "./ProcessorHealth";
import { ageLabel, processorLanes, processorSubtitle } from "./processorFace";

const AGE_TICK_MS = 1_000;
const PATTERN_PX = 120;

export function BeamformerFace({ node }: { node: PatchNode }) {
  const workspace = useWorkspaceContext();
  const state = useProcessorStore((store) => store.byNode[node.id]);
  const array = arrayOf(workspace.resolved, node.id);
  const status = useArrayStore((store) => (array === null ? undefined : store.byNode[array]));
  const now = useNow(AGE_TICK_MS);
  if (node.kind !== "beamformer") {
    return null;
  }
  const reading = readingOf(state, "beamformer");
  const period = Number(nodeSettings(workspace.context.catalog, node)?.update_ms ?? 0);
  const stale = state !== undefined && isStale(state.receivedAt, now, period);
  const health = processorStatusOf(status, node.id);
  const lanes = Math.max(
    processorLanes(workspace.resolved, array, status),
    reading?.weights.length ?? 0,
  );
  return (
    <NodeShell
      node={node}
      title="Beamformer"
      category="tool"
      subtitle={processorSubtitle(workspace.resolved, node.id, status, state, now, period)}
    >
      <FaceBody>
        <div className={`flex gap-2 p-2 ${stale ? "opacity-45" : ""}`}>
          <BeamPattern reading={reading} />
          <div className="min-w-0 flex-1">
            <WeightBars reading={reading} />
          </div>
        </div>
        <NodeForm node={node} lanes={lanes} />
        <StatusGrid readouts={readoutsOf(state)} age={ageLabel(state?.receivedAt, now)} />
        <ProcessorError status={health} />
      </FaceBody>
      <ProcessorFooter status={health} chips={beamChips(reading)} />
    </NodeShell>
  );
}

function BeamPattern({ reading }: { reading: BeamformerReading | null }) {
  const centre = PATTERN_PX / 2;
  const radius = centre - 6;
  const pattern = reading?.pattern ?? [];
  const steer = reading?.steer_deg ?? null;
  return (
    <svg
      role="img"
      aria-label="Beam pattern"
      width={PATTERN_PX}
      height={PATTERN_PX}
      viewBox={`0 0 ${PATTERN_PX} ${PATTERN_PX}`}
      className="shrink-0"
    >
      <circle cx={centre} cy={centre} r={radius} className="fill-well stroke-line" />
      <circle cx={centre} cy={centre} r={radius / 2} className="fill-none stroke-line/60" />
      <text x={centre} y={9} textAnchor="middle" className="fill-ink-faint font-mono text-plot">
        Fwd
      </text>
      {pattern.length > 0 && (
        <path d={patternPath(pattern, radius, centre)} className="fill-accent/25 stroke-accent" />
      )}
      {steer !== null && (
        <path
          d={polarTick(steer, radius - 8, radius + 4, centre)}
          className="stroke-accent"
          strokeWidth={2}
        />
      )}
      {(reading?.nulls_deg ?? []).map((deg) => (
        <path
          key={deg}
          d={polarTick(deg, radius - 6, radius + 4, centre)}
          className={
            steer !== null && angleGap(deg, steer) < NULL_CLOSE_DEG
              ? "stroke-warn"
              : "stroke-danger"
          }
          strokeWidth={1.5}
        />
      ))}
    </svg>
  );
}

function WeightBars({ reading }: { reading: BeamformerReading | null }) {
  const weights = reading?.weights ?? [];
  if (weights.length === 0) {
    return null;
  }
  const percent = amplitudePercent(weights);
  return (
    <div role="group" className="flex flex-col gap-0.5" aria-label="Lane weights">
      {weights.map((weight, lane) => (
        <div key={lane} className="flex items-center gap-1.5" title={weightTitle(lane, weight)}>
          <span className="w-4 font-mono text-2xs text-hue-indigo">{lane + 1}</span>
          <MeterBar
            label={`Lane ${lane + 1} weight`}
            value={(percent[lane] ?? 0) / 100}
            valueText={weightTitle(lane, weight)}
          />
        </div>
      ))}
    </div>
  );
}
