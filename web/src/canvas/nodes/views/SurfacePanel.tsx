import { useState } from "react";
import { ChoiceChip } from "../../../components/face/Chips";
import { Readout, Readouts } from "../../../components/face/Readouts";
import { Segmented } from "../../../components/Segmented";
import type { Colormap } from "../../../gl/surface";
import type { StreamKind, SurfaceView, ViewLayer } from "../../../lib/types";
import { type BearingFrame, frameOptions, shownFrame } from "./bearing";
import { RangeDopplerPlot } from "./RangeDopplerPlot";
import { SpatialPlot } from "./SpatialPlot";
import { type SpatialView, VIEW_OPTIONS } from "./spatialSpectrum";
import { FringeLine, VisibilityPlots } from "./VisibilityPlots";
import { layerOptions, withFringe } from "./visibility";

export interface SurfaceProps {
  node: string;
  known: boolean;
  view: SurfaceView | null;
  dim: boolean;
  colormap: Colormap;
  picked: number | null;
  receivedAt: number | undefined;
}

/** Surfaces drawn as a coloured grid, so the face offers a colour choice. */
export const GRID_STREAMS: readonly StreamKind[] = ["range_doppler", "spatial_spectrum"];

export function SurfacePanel({ stream, ...props }: SurfaceProps & { stream: StreamKind }) {
  switch (stream) {
    case "range_doppler":
      return (
        <RangeDopplerPlot
          node={props.node}
          known={props.known}
          marks={props.view?.marks ?? []}
          dim={props.dim}
          colormap={props.colormap}
          picked={props.picked}
        />
      );
    case "spatial_spectrum":
      return <SpatialPanel {...props} />;
    case "visibility":
      return <VisibilityPanel {...props} />;
    default:
      return null;
  }
}

function SpatialPanel({ node, known, view, dim, colormap }: SurfaceProps) {
  const [picked, setPicked] = useState<BearingFrame | null>(null);
  const [shown, setShown] = useState<SpatialView>("map");
  const trueKnown = view?.heading_deg != null;
  const frame = shownFrame(picked, trueKnown);
  return (
    <>
      <div className="flex shrink-0 flex-wrap items-center gap-2 px-2 py-1.5">
        <Segmented label="View" value={shown} options={VIEW_OPTIONS} onChange={setShown} />
        <Segmented
          label="Bearing frame"
          value={frame}
          options={frameOptions(trueKnown)}
          onChange={setPicked}
        />
      </div>
      <SpatialPlot
        node={node}
        known={known}
        view={shown}
        bearingFrame={frame}
        offsetDeg={frame === "true" ? (view?.heading_deg ?? 0) : 0}
        colormap={colormap}
        marks={view?.marks ?? []}
        dim={dim}
      />
    </>
  );
}

interface Fringe {
  key: string;
  index: number;
  history: readonly number[];
}

function useFringe(layer: ViewLayer | undefined, index: number, at: number | undefined) {
  const [fringe, setFringe] = useState<Fringe>({ key: "", index, history: [] });
  const key = `${index}:${at ?? ""}`;
  if (layer?.phase_deg != null && fringe.key !== key) {
    const phase = layer.phase_deg;
    const history = fringe.index === index ? withFringe(fringe.history, phase) : [phase];
    setFringe({ key, index, history });
    return history;
  }
  return fringe.index === index ? fringe.history : [];
}

function VisibilityPanel({ node, known, view, dim, receivedAt }: SurfaceProps) {
  const [chosen, setChosen] = useState(0);
  const layers = view?.layers ?? [];
  const index = chosen < layers.length ? chosen : 0;
  const layer = layers[index];
  const fringe = useFringe(layer, index, receivedAt);
  return (
    <>
      <VisibilityPlots node={node} known={known} index={index} dim={dim} />
      {layer !== undefined && (
        <div className="flex shrink-0 items-center gap-2 border-t border-line pr-2">
          {layers.length > 1 && (
            <ChoiceChip
              label="Baseline"
              title="Baseline shown"
              value={index}
              options={layerOptions(layers)}
              onChange={setChosen}
            />
          )}
          <Readouts ruled={false} className="min-w-0 flex-1">
            <Readout label={layer.label} title={layer.title}>
              {layer.text}
            </Readout>
          </Readouts>
          <FringeLine history={fringe} />
        </div>
      )}
    </>
  );
}
