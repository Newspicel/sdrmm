import { useState } from "react";
import { MapPanel } from "../components/MapPanel";
import { recordEvent } from "../lib/diagnostics";
import type { FusionGridFrame } from "../lib/frame";
import { useSinkFeed } from "../lib/sinks";
import { useSurface, useSurfaceRefusal } from "../lib/useSurface";

interface HeldHeat {
  node: string;
  frame: FusionGridFrame;
}

function HeatFeed({ node, onFrame }: { node: string; onFrame: (held: HeldHeat) => void }) {
  useSurface(node, (surface) => {
    if (surface.kind === "fusion_grid") {
      onFrame({ node, frame: surface.frame });
    } else {
      recordEvent("warn", "map", `unexpected ${surface.kind} surface`);
    }
  });
  return null;
}

export function MapView({
  node,
  sourceName,
  active,
}: {
  node: string;
  sourceName: (source: string) => string;
  active: boolean;
}) {
  const feed = useSinkFeed(node);
  const [held, setHeld] = useState<HeldHeat | null>(null);
  const surface = feed.surfaces.find((ref) => ref.kind === "fusion_grid")?.node ?? null;
  const heatRefused = useSurfaceRefusal(surface);
  return (
    <>
      {surface !== null && <HeatFeed key={surface} node={surface} onFrame={setHeld} />}
      <MapPanel
        feed={{ entities: feed.entities, tracks: feed.tracks, sourceName }}
        heat={surface === null ? undefined : held?.node === surface ? held.frame : null}
        heatRefused={heatRefused}
        active={active}
        className="h-full min-h-0 w-full flex-1"
      />
    </>
  );
}
