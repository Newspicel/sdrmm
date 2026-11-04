import { processorStatusOf, useArrayStore } from "../../lib/arrays";
import { isStale, readingOf, readoutsOf, useProcessorStore } from "../../lib/processors";
import type { PatchNode, PolarimeterReading } from "../../lib/types";
import { useNow } from "../../lib/useNow";
import { useWorkspaceContext } from "../context";
import { arrayOf } from "../lanes";
import { NodeForm, nodeSettings, StatusGrid } from "./NodeForm";
import { FaceBody, NodeShell } from "./NodeShell";
import { ProcessorFooter } from "./ProcessorFooter";
import { ProcessorError } from "./ProcessorHealth";
import { ellipsePath, senseArrow } from "./polarimeter";
import { ageLabel, processorLanes, processorSubtitle } from "./processorFace";

const AGE_TICK_MS = 1_000;
const GLYPH_PX = 96;

export function PolarimeterFace({ node }: { node: PatchNode }) {
  const workspace = useWorkspaceContext();
  const state = useProcessorStore((store) => store.byNode[node.id]);
  const array = arrayOf(workspace.resolved, node.id);
  const status = useArrayStore((store) => (array === null ? undefined : store.byNode[array]));
  const now = useNow(AGE_TICK_MS);
  if (node.kind !== "polarimeter") {
    return null;
  }
  const reading = readingOf(state, "polarimeter");
  const period = Number(nodeSettings(workspace.context.catalog, node)?.report_ms ?? 0);
  const stale = state !== undefined && isStale(state.receivedAt, now, period);
  const processor = processorStatusOf(status, node.id);
  return (
    <NodeShell
      node={node}
      title="Polarimeter"
      category="tool"
      subtitle={processorSubtitle(workspace.resolved, node.id, status, state, now, period)}
    >
      <FaceBody>
        <div className={`flex items-center gap-1 pl-2 ${stale ? "opacity-45" : ""}`}>
          <PolarGlyph reading={reading} />
          <div className="min-w-0 flex-1">
            <StatusGrid readouts={readoutsOf(state)} age={ageLabel(state?.receivedAt, now)} />
          </div>
        </div>
        <NodeForm node={node} lanes={processorLanes(workspace.resolved, array, status)} />
        <ProcessorError status={processor} />
      </FaceBody>
      <ProcessorFooter status={processor} />
    </NodeShell>
  );
}

function PolarGlyph({ reading }: { reading: PolarimeterReading | null }) {
  const centre = GLYPH_PX / 2;
  const radius = centre - 8;
  const angle = reading?.angle_deg ?? 0;
  const chi = reading?.ellipticity_deg ?? 0;
  const arrow = reading === null ? null : senseArrow(angle, chi, reading.v, radius, centre);
  return (
    <svg
      role="img"
      aria-label="Polarisation"
      width={GLYPH_PX}
      height={GLYPH_PX}
      viewBox={`0 0 ${GLYPH_PX} ${GLYPH_PX}`}
      className="my-2 shrink-0 rounded-control bg-well"
    >
      <path
        d={`M4 ${centre}H${GLYPH_PX - 4}M${centre} 4V${GLYPH_PX - 4}`}
        className="stroke-line"
      />
      <text
        x={GLYPH_PX - 4}
        y={centre - 3}
        textAnchor="end"
        className="fill-ink-faint font-mono text-plot"
      >
        H
      </text>
      <text x={centre + 3} y={10} className="fill-ink-faint font-mono text-plot">
        V
      </text>
      {reading !== null && (
        <>
          <path
            d={ellipsePath(angle, chi, radius, centre)}
            className="fill-accent/15 stroke-accent"
            strokeWidth={1.5}
          />
          {arrow !== null && (
            <>
              <path d={arrow.arc} className="fill-none stroke-accent" strokeWidth={2.5} />
              <path d={arrow.head} className="fill-accent stroke-accent" strokeLinejoin="round" />
            </>
          )}
        </>
      )}
    </svg>
  );
}
