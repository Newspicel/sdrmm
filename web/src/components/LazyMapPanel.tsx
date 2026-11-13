import { type ComponentProps, lazy, Suspense } from "react";
import type { MapPanel as EagerMapPanel } from "./MapPanel";

const Panel = lazy(() => import("./MapPanel").then((module) => ({ default: module.MapPanel })));

/// The map panel, fetching maplibre (about 1 MB) only once a map is first shown.
export function MapPanel(props: ComponentProps<typeof EagerMapPanel>) {
  return (
    <Suspense
      fallback={
        <div className={`bg-bg ${props.className ?? "h-[min(60dvh,28rem)] min-h-64 w-full"}`} />
      }
    >
      <Panel {...props} />
    </Suspense>
  );
}
