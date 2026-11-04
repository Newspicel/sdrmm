import { useState } from "react";
import { Rose } from "../../components/Rose";
import { Segmented } from "../../components/Segmented";
import { processorStatusOf, useArrayStore } from "../../lib/arrays";
import { isStale, readingOf, readoutsOf, useProcessorStore } from "../../lib/processors";
import type { PatchNode } from "../../lib/types";
import { useNow } from "../../lib/useNow";
import { useWorkspaceContext } from "../context";
import { arrayOf } from "../lanes";
import {
  type BearingFrame,
  dfChips,
  frameOptions,
  frameRotation,
  roseLetters,
  roseNeedles,
  roseWedge,
  shownFrame,
} from "./df";
import { NodeForm, nodeSettings, StatusGrid } from "./NodeForm";
import { FaceBody, NodeShell } from "./NodeShell";
import { ProcessorFooter } from "./ProcessorFooter";
import { ProcessorError } from "./ProcessorHealth";
import { ageLabel, processorGate, processorLanes, processorSubtitle } from "./processorFace";

const AGE_TICK_MS = 1_000;
const ROSE_PX = 220;

export function DfFace({ node }: { node: PatchNode }) {
  const workspace = useWorkspaceContext();
  const state = useProcessorStore((store) => store.byNode[node.id]);
  const array = arrayOf(workspace.resolved, node.id);
  const status = useArrayStore((store) => (array === null ? undefined : store.byNode[array]));
  const now = useNow(AGE_TICK_MS);
  const [picked, setPicked] = useState<BearingFrame | null>(null);
  if (node.kind !== "df") {
    return null;
  }
  const reportMs = Number(nodeSettings(workspace.context.catalog, node)?.report_ms ?? 0);
  const reading = readingOf(state, "df");
  const stale = state !== undefined && isStale(state.receivedAt, now, reportMs);
  const gate = processorGate(status, node.id);
  const trueAvailable = reading?.azimuth_deg != null;
  const frame = shownFrame(picked, trueAvailable);
  const peaks = reading?.peaks ?? [];
  const processor = processorStatusOf(status, node.id);
  return (
    <NodeShell
      node={node}
      title="Direction finder"
      category="tool"
      subtitle={processorSubtitle(workspace.resolved, node.id, status, state, now, reportMs)}
    >
      <FaceBody>
        <div className="flex flex-col items-center gap-2 p-2">
          <Segmented
            label="Bearing frame"
            value={frame}
            options={frameOptions(trueAvailable)}
            onChange={setPicked}
          />
          <Rose
            label="Bearing rose"
            size={ROSE_PX}
            marks={roseLetters(frame === "true")}
            spectrum={reading?.pseudospectrum ?? []}
            rotateDeg={frameRotation(reading, frame)}
            needles={roseNeedles(peaks, frame)}
            wedge={roseWedge(peaks, frame)}
            tickDeg={frame === "true" ? (reading?.azimuth_deg ?? null) : null}
            dim={stale || gate !== null}
          />
        </div>
        <NodeForm node={node} lanes={processorLanes(workspace.resolved, array, status)} />
        <StatusGrid readouts={readoutsOf(state)} age={ageLabel(state?.receivedAt, now)} />
        <ProcessorError status={processor} />
      </FaceBody>
      <ProcessorFooter status={processor} chips={dfChips(reading, gate, stale)} />
    </NodeShell>
  );
}
