import { useState } from "react";
import { Rose } from "../../../components/Rose";
import { Segmented } from "../../../components/Segmented";
import type { BearingView } from "../../../lib/types";
import {
  type BearingFrame,
  frameOptions,
  frameRotation,
  roseLetters,
  roseNeedles,
  roseWedge,
  shownFrame,
} from "./bearing";

const ROSE_PX = 220;

export function BearingRose({ view, dim }: { view: BearingView | null; dim: boolean }) {
  const [picked, setPicked] = useState<BearingFrame | null>(null);
  const trueAvailable = view?.heading_deg != null;
  const frame = shownFrame(picked, trueAvailable);
  return (
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
        spectrum={view?.spectrum ?? []}
        rotateDeg={frameRotation(view, frame)}
        needles={roseNeedles(view, frame)}
        wedge={roseWedge(view, frame)}
        tickDeg={frame === "true" ? (view?.heading_deg ?? null) : null}
        dim={dim}
      />
    </div>
  );
}
