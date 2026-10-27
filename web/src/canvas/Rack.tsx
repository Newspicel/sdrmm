import { memo, useCallback, useEffect, useRef, useState } from "react";

import { Button } from "../components/BaseControls";
import { deepEqual } from "../lib/deepEqual";
import type { PatchNode, RackLayout } from "../lib/types";
import { useWorkspaceContext } from "./context";
import {
  clampCells,
  moveSlot,
  placeSlot,
  RACK_COLS,
  RACK_ROWS,
  type RackEdge,
  resizeSlot,
  slotRoom,
} from "./graph";
import { FACES, faceSize } from "./nodes";

const SETTLE_MS = 500;

const NUDGES: Record<string, { dx: number; dy: number }> = {
  ArrowLeft: { dx: -1, dy: 0 },
  ArrowRight: { dx: 1, dy: 0 },
  ArrowUp: { dx: 0, dy: -1 },
  ArrowDown: { dx: 0, dy: 1 },
};

interface Preview {
  layout: RackLayout;
  dropped: boolean;
}

interface Gesture {
  node: string;
  mode: "move" | "corner" | RackEdge;
  originX: number;
  originY: number;
  base: RackLayout;
}

export function Rack() {
  const workspace = useWorkspaceContext();
  const hostRef = useRef<HTMLDivElement>(null);
  const gesture = useRef<Gesture | null>(null);
  const [preview, setPreview] = useState<Preview | null>(null);

  const landed = preview !== null && preview.dropped && sameRack(workspace.rack, preview.layout);

  useEffect(() => {
    if (preview === null || !preview.dropped) {
      return;
    }
    const settle = setTimeout(() => setPreview(null), SETTLE_MS);
    return () => clearTimeout(settle);
  }, [preview]);

  const cellSize = useCallback(() => {
    const host = hostRef.current;
    return host === null
      ? { w: 1, h: 1 }
      : { w: host.clientWidth / RACK_COLS, h: host.clientHeight / RACK_ROWS };
  }, []);

  const onPointerMove = useCallback(
    (event: React.PointerEvent) => {
      const active = gesture.current;
      if (active === null) {
        return;
      }
      const cell = cellSize();
      const dx = Math.round((event.clientX - active.originX) / cell.w);
      const dy = Math.round((event.clientY - active.originY) / cell.h);
      const next = applyGesture(active, dx, dy);
      setPreview((current) =>
        current !== null && sameRack(current.layout, next)
          ? current
          : { layout: next, dropped: false },
      );
    },
    [cellSize],
  );

  const endGesture = useCallback(
    (event: React.PointerEvent) => {
      const active = gesture.current;
      gesture.current = null;
      if (active === null) {
        setPreview(null);
        return;
      }
      const cell = cellSize();
      const dx = Math.round((event.clientX - active.originX) / cell.w);
      const dy = Math.round((event.clientY - active.originY) / cell.h);
      if (dx === 0 && dy === 0) {
        setPreview(null);
        return;
      }
      setPreview({ layout: applyGesture(active, dx, dy), dropped: true });
      workspace.edit((snapshot) => ({
        ...snapshot,
        rack: applyGesture({ ...active, base: snapshot.rack ?? {} }, dx, dy),
      }));
    },
    [cellSize, workspace],
  );

  const begin = (event: React.PointerEvent, node: string, mode: Gesture["mode"]) => {
    if (event.button !== 0) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    gesture.current = {
      node,
      mode,
      originX: event.clientX,
      originY: event.clientY,
      base: workspace.rack,
    };
  };

  const nudge = (event: React.KeyboardEvent, node: string, mode: Gesture["mode"]) => {
    const step = NUDGES[event.key];
    if (step === undefined) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    workspace.edit((snapshot) => ({
      ...snapshot,
      rack: applyGesture(
        { node, mode, originX: 0, originY: 0, base: snapshot.rack ?? {} },
        step.dx,
        step.dy,
      ),
    }));
  };

  const slots = (landed ? workspace.rack : (preview?.layout ?? workspace.rack)).slots ?? [];
  if (slots.length === 0) {
    return (
      <div className="flex min-h-0 flex-1 items-center justify-center bg-bg">
        <p className="text-sm text-ink-dim">Nothing pinned.</p>
      </div>
    );
  }

  return (
    <div
      ref={hostRef}
      className="grid min-h-0 flex-1 gap-px bg-bg p-px"
      style={{
        gridTemplateColumns: `repeat(${RACK_COLS}, minmax(0, 1fr))`,
        gridTemplateRows: `repeat(${RACK_ROWS}, minmax(0, 1fr))`,
      }}
      onPointerMove={onPointerMove}
      onPointerUp={endGesture}
      onPointerCancel={endGesture}
    >
      {slots.map((slot) => {
        const node = workspace.graph.nodes.find((candidate) => candidate.id === slot.node);
        if (node === undefined) {
          return null;
        }
        return (
          <div
            key={slot.node}
            data-id={slot.node}
            className="flex min-h-0 min-w-0 items-center justify-center"
            style={{
              gridColumn: `${slot.x + 1} / span ${slot.w}`,
              gridRow: `${slot.y + 1} / span ${slot.h}`,
            }}
          >
            <div className="relative max-h-full max-w-full" style={faceSize(node)}>
              <RackFace node={node} />
              <Grips node={slot.node} onBegin={begin} onNudge={nudge} />
            </div>
          </div>
        );
      })}
    </div>
  );
}

const RackFace = memo(function RackFace({ node }: { node: PatchNode }) {
  const Face = FACES[node.kind];
  return <Face node={node} />;
});

function Grips({
  node,
  onBegin,
  onNudge,
}: {
  node: string;
  onBegin: (event: React.PointerEvent, node: string, mode: Gesture["mode"]) => void;
  onNudge: (event: React.KeyboardEvent, node: string, mode: Gesture["mode"]) => void;
}) {
  const grip = (mode: Gesture["mode"], className: string, label: string, hint: string) => (
    <Button
      type="button"
      aria-label={`${label} ${node}`}
      aria-keyshortcuts="ArrowUp ArrowDown ArrowLeft ArrowRight"
      title={hint}
      className={`absolute ${className}`}
      onPointerDown={(event) => onBegin(event, node, mode)}
      onKeyDown={(event) => onNudge(event, node, mode)}
    />
  );
  return (
    <>
      {grip(
        "n",
        "inset-x-0 top-0 h-1.5 cursor-ns-resize",
        "Boundary above",
        "Drag the boundary above",
      )}
      {grip(
        "s",
        "inset-x-0 bottom-0 h-1.5 cursor-ns-resize",
        "Boundary below",
        "Drag the boundary below",
      )}
      {grip(
        "w",
        "inset-y-0 left-0 w-1.5 cursor-ew-resize",
        "Boundary left of",
        "Drag the boundary to the left",
      )}
      {grip(
        "e",
        "inset-y-0 right-0 w-1.5 cursor-ew-resize",
        "Boundary right of",
        "Drag the boundary to the right",
      )}
      {grip(
        "move",
        "top-1.5 right-14 left-1.5 h-5 cursor-move",
        "Move",
        "Move: drop on another face to trade places",
      )}
      {grip(
        "corner",
        "right-0 bottom-0 size-3.5 cursor-se-resize border-r-2 border-b-2 border-line-strong",
        "Resize",
        "Resize",
      )}
    </>
  );
}

function applyGesture(gesture: Gesture, dx: number, dy: number): RackLayout {
  const slot = (gesture.base.slots ?? []).find((candidate) => candidate.node === gesture.node);
  if (slot === undefined) {
    return gesture.base;
  }
  switch (gesture.mode) {
    case "move":
      return moveSlot(gesture.base, gesture.node, {
        x: clamp(slot.x + dx, 0, RACK_COLS - slot.w),
        y: clamp(slot.y + dy, 0, RACK_ROWS - slot.h),
      });
    case "corner":
      return placeSlot(gesture.base, gesture.node, {
        x: slot.x,
        y: slot.y,
        w: clamp(slot.w + dx, 1, RACK_COLS - slot.x),
        h: clamp(slot.h + dy, 1, RACK_ROWS - slot.y),
      });
    default: {
      const edge = gesture.mode;
      const cells = edge === "n" || edge === "s" ? dy : dx;
      return resizeSlot(
        gesture.base,
        gesture.node,
        edge,
        clampCells(slotRoom(gesture.base, gesture.node, edge), cells),
      );
    }
  }
}

function clamp(value: number, low: number, high: number): number {
  return Math.min(Math.max(value, low), Math.max(high, low));
}

function sameRack(a: RackLayout, b: RackLayout): boolean {
  return deepEqual(a.slots ?? [], b.slots ?? []);
}
