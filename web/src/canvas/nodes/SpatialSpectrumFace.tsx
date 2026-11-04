import { useState } from "react";
import { ChoiceChip } from "../../components/face/Chips";
import { ColourScale } from "../../components/PlotAxis";
import { COLOUR_OPTIONS } from "../../components/plotFrame";
import { Segmented } from "../../components/Segmented";
import { type Colormap, DEFAULT_COLORMAP } from "../../gl/surface";
import { processorStatusOf, useArrayStore } from "../../lib/arrays";
import { isStale, readingOf, readoutsOf, useProcessorStore } from "../../lib/processors";
import type { PatchNode, SpatialReading } from "../../lib/types";
import { useNow } from "../../lib/useNow";
import { useWorkspaceContext } from "../context";
import { arrayOf } from "../lanes";
import { NodeForm, nodeSettings, StatusGrid } from "./NodeForm";
import { FaceBody, NodeShell } from "./NodeShell";
import { ProcessorFooter } from "./ProcessorFooter";
import { type Chip, ProcessorError } from "./ProcessorHealth";
import { ageLabel, processorSubtitle } from "./processorFace";
import { SpatialPlot } from "./SpatialPlot";
import {
  type BearingFrame,
  frameOffset,
  frameOptions,
  type SpatialView,
  VIEW_OPTIONS,
} from "./spatialSpectrum";

const AGE_TICK_MS = 1_000;

export function SpatialSpectrumFace({ node }: { node: PatchNode }) {
  const workspace = useWorkspaceContext();
  const state = useProcessorStore((store) => store.byNode[node.id]);
  const array = arrayOf(workspace.resolved, node.id);
  const status = useArrayStore((store) => (array === null ? undefined : store.byNode[array]));
  const now = useNow(AGE_TICK_MS);
  const [chosenFrame, setChosenFrame] = useState<BearingFrame | null>(null);
  const [view, setView] = useState<SpatialView>("map");
  const [colormap, setColormap] = useState<Colormap>(DEFAULT_COLORMAP);
  if (node.kind !== "spatial_spectrum") {
    return null;
  }
  const settings = nodeSettings(workspace.context.catalog, node);
  const reading = readingOf(state, "spatial_spectrum");
  const period = Number(settings?.report_ms ?? 0);
  const stale = state !== undefined && isStale(state.receivedAt, now, period);
  const trueKnown = reading?.azimuth_deg != null;
  const bearingFrame: BearingFrame = trueKnown ? (chosenFrame ?? "true") : "relative";
  const span = Number(settings?.span_db ?? 0);
  const offsetDeg = frameOffset(bearingFrame, reading);
  const processor = processorStatusOf(status, node.id);
  return (
    <NodeShell
      node={node}
      title="Spatial spectrum"
      category="tool"
      subtitle={processorSubtitle(workspace.resolved, node.id, status, state, now, period)}
    >
      <FaceBody scroll={false}>
        <div className="flex min-h-0 flex-1 flex-col">
          <div className="flex shrink-0 flex-wrap items-center gap-2 px-2 py-1.5">
            <Segmented label="View" value={view} options={VIEW_OPTIONS} onChange={setView} />
            <Segmented
              label="Bearing frame"
              value={bearingFrame}
              options={frameOptions(trueKnown)}
              onChange={setChosenFrame}
            />
            {view === "map" && (
              <span className="ml-auto" title="Below the strongest cell">
                <ColourScale colormap={colormap} min={-span} max={0} unit="dB" />
              </span>
            )}
          </div>
          <SpatialPlot
            node={node.id}
            known={processor !== null}
            view={view}
            bearingFrame={bearingFrame}
            offsetDeg={offsetDeg}
            colormap={colormap}
            peaks={reading?.peaks ?? []}
            dim={stale || reading === null}
          />
          <NodeForm
            node={node}
            lead={
              <ChoiceChip
                label="Colours"
                title="Colours"
                value={colormap}
                options={COLOUR_OPTIONS}
                onChange={setColormap}
              />
            }
          />
          <StatusGrid
            readouts={readoutsOf(state)}
            age={ageLabel(state?.receivedAt, now)}
            columns={1}
          />
          <ProcessorError status={processor} />
        </div>
      </FaceBody>
      <ProcessorFooter status={processor} chips={droppedChips(reading)} />
    </NodeShell>
  );
}

function droppedChips(reading: SpatialReading | null): Chip[] {
  const dropped = reading?.dropped_frames ?? 0;
  return dropped > 0 ? [{ label: `Drops ${dropped}`, title: "Frames lost", danger: true }] : [];
}
