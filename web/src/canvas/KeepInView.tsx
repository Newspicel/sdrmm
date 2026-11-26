import { useStore, useStoreApi } from "@xyflow/react";
import { useEffect } from "react";
import { useReveal } from "./placement";

type FlowState = Parameters<Parameters<typeof useStore>[0]>[0];

const added = new Set<string>();

/** Marks a node just added and inspected, so it stays on screen once it has a size and the
 * canvas has narrowed for the inspector. */
export function revealWhenInspected(node: string): void {
  added.add(node);
}

/** Brings a just added, inspected `node` on screen. A face clicked while in use never moves.
 * Renders nothing, so its subscriptions redraw no face. */
export function KeepInView({ node }: { node: string | null }) {
  const reveal = useReveal();
  const store = useStoreApi();
  const paneWidth = useStore((state) => state.width);
  const paneHeight = useStore((state) => state.height);
  const width = useStore((state) => measured(state, node)?.width);
  const height = useStore((state) => measured(state, node)?.height);

  useEffect(
    () => () => {
      if (node !== null) {
        added.delete(node);
      }
    },
    [node],
  );

  useEffect(() => {
    if (node === null || !added.has(node) || paneWidth <= 0 || paneHeight <= 0) {
      return;
    }
    const at = store.getState().nodeLookup.get(node)?.internals.positionAbsolute;
    if (at === undefined || width === undefined || height === undefined) {
      return;
    }
    reveal({ x: at.x, y: at.y, w: width, h: height });
  }, [node, paneWidth, paneHeight, width, height, reveal, store]);

  return null;
}

function measured(state: FlowState, id: string | null) {
  return id === null ? undefined : state.nodeLookup.get(id)?.measured;
}
