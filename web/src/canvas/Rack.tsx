import { memo, useCallback, useEffect, useRef, useState } from "react";

import { Button } from "../components/BaseControls";
import { deepEqual } from "../lib/deepEqual";
import type { PatchNode, RackLayout } from "../lib/types";
import { useWorkspaceContext } from "./context";
import { RACK_COLS, RACK_ROWS, type RackCell } from "./graph";
import { FACES, rackFaceSize } from "./nodes";
import {
  applyGesture,
  fromHeader,
  type Gesture,
  type GestureMode,
  type Ghost,
  moveGhost,
} from "./rackGesture";

const SETTLE_MS = 500;
const DRAG_PX = 4;

const NUDGES: Record<string, { dx: number; dy: number }> = {
  ArrowLeft: { dx: -1, dy: 0 },
  ArrowRight: { dx: 1, dy: 0 },
  ArrowUp: { dx: 0, dy: -1 },
  ArrowDown: { dx: 0, dy: 1 },
};

interface Preview {
  layout: RackLayout;
  dropped: boolean;
  ghost: Ghost | null;
}

/// Pointer capture waits for real movement so a click or double-click on the title still lands.
interface Drag extends Gesture {
  originX: number;
  originY: number;
  pointer: number;
  handle: Element;
  captured: boolean;
}

export function Rack() {
  const workspace = useWorkspaceContext();
  const hostRef = useRef<HTMLDivElement>(null);
  const drag = useRef<Drag | null>(null);
  const [preview, setPreview] = useState<Preview | null>(null);

  const landed = preview !== null && preview.dropped && sameRack(workspace.rack, preview.layout);

  useEffect(() => {
    if (preview === null || !preview.dropped) {
      return;
    }
    const settle = setTimeout(() => setPreview(null), SETTLE_MS);
    return () => clearTimeout(settle);
  }, [preview]);

  const cells = useCallback((active: Drag, event: React.PointerEvent) => {
    const host = hostRef.current;
    const w = host === null ? 1 : host.clientWidth / RACK_COLS;
    const h = host === null ? 1 : host.clientHeight / RACK_ROWS;
    return {
      dx: Math.round((event.clientX - active.originX) / w),
      dy: Math.round((event.clientY - active.originY) / h),
    };
  }, []);

  const endGesture = useCallback(
    (event: React.PointerEvent) => {
      const active = drag.current;
      drag.current = null;
      const { dx, dy } = active === null ? { dx: 0, dy: 0 } : cells(active, event);
      if (active === null || !active.captured || (dx === 0 && dy === 0)) {
        setPreview(null);
        return;
      }
      setPreview({ layout: applyGesture(active, dx, dy), dropped: true, ghost: null });
      workspace.edit((snapshot) => ({
        ...snapshot,
        rack: applyGesture({ ...active, base: snapshot.rack ?? {} }, dx, dy),
      }));
    },
    [cells, workspace],
  );

  const onPointerMove = useCallback(
    (event: React.PointerEvent) => {
      const active = drag.current;
      if (active === null) {
        return;
      }
      if (event.buttons === 0) {
        endGesture(event);
        return;
      }
      if (!active.captured) {
        if (Math.hypot(event.clientX - active.originX, event.clientY - active.originY) < DRAG_PX) {
          return;
        }
        active.handle.setPointerCapture(active.pointer);
        active.captured = true;
      }
      const { dx, dy } = cells(active, event);
      const layout = applyGesture(active, dx, dy);
      const next = { layout, dropped: false, ghost: moveGhost(active, dx, dy, layout) };
      setPreview((current) => (current !== null && samePreview(current, next) ? current : next));
    },
    [cells, endGesture],
  );

  const begin = (event: React.PointerEvent, node: string, mode: GestureMode) => {
    if (event.button !== 0) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    drag.current = {
      node,
      mode,
      base: workspace.rack,
      originX: event.clientX,
      originY: event.clientY,
      pointer: event.pointerId,
      handle: event.currentTarget,
      captured: false,
    };
  };

  const pickUp = (event: React.PointerEvent, node: string) => {
    if (fromHeader(event.target)) {
      begin(event, node, "move");
    }
  };

  const nudge = (event: React.KeyboardEvent, node: string, mode: GestureMode) => {
    const step = NUDGES[event.key];
    if (step === undefined) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    workspace.edit((snapshot) => ({
      ...snapshot,
      rack: applyGesture({ node, mode, base: snapshot.rack ?? {} }, step.dx, step.dy),
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
  const ghost = landed ? null : (preview?.ghost ?? null);

  return (
    <div
      ref={hostRef}
      className="grid min-h-0 flex-1 gap-px overflow-hidden bg-bg p-px"
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
            className={`relative min-h-0 min-w-0 rounded-control has-[>button:focus-visible]:outline-2 has-[>button:focus-visible]:outline-accent [&_header]:cursor-grab ${
              ghost?.partner === slot.node ? "outline-2 outline-accent" : ""
            }`}
            style={area(slot)}
            onPointerDown={(event) => pickUp(event, slot.node)}
          >
            <div className="size-full overflow-auto">
              <div className="h-full" style={rackFaceSize(node)}>
                <RackFace node={node} />
              </div>
            </div>
            <Grips node={slot.node} onBegin={begin} onNudge={nudge} />
          </div>
        );
      })}
      {ghost !== null && (
        <div
          aria-hidden
          data-ghost={ghost.legal ? "legal" : "blocked"}
          className={`pointer-events-none z-20 rounded-control border-2 border-dashed ${
            ghost.legal ? "border-accent bg-accent/10" : "border-danger bg-danger/10"
          }`}
          style={area(ghost.cell)}
        />
      )}
    </div>
  );
}

const RackFace = memo(function RackFace({ node }: { node: PatchNode }) {
  const Face = FACES[node.kind];
  return <Face node={node} />;
});

const EDGE_LINE =
  "before:absolute before:bg-accent before:opacity-0 before:transition-opacity hover:before:opacity-100";

function Grips({
  node,
  onBegin,
  onNudge,
}: {
  node: string;
  onBegin: (event: React.PointerEvent, node: string, mode: GestureMode) => void;
  onNudge: (event: React.KeyboardEvent, node: string, mode: GestureMode) => void;
}) {
  const grip = (mode: GestureMode, className: string, label: string, hint: string) => (
    <Button
      type="button"
      aria-label={`${label} ${node}`}
      aria-keyshortcuts="ArrowUp ArrowDown ArrowLeft ArrowRight"
      title={hint}
      className={`absolute z-10 ${className}`}
      onPointerDown={(event) => onBegin(event, node, mode)}
      onKeyDown={(event) => onNudge(event, node, mode)}
    />
  );
  return (
    <>
      {grip(
        "n",
        `inset-x-3 -top-1 h-2 cursor-ns-resize ${EDGE_LINE} before:inset-x-0 before:top-[3px] before:h-0.5`,
        "Boundary above",
        "Drag the boundary above",
      )}
      {grip(
        "s",
        `inset-x-3 -bottom-1 h-2 cursor-ns-resize ${EDGE_LINE} before:inset-x-0 before:top-[3px] before:h-0.5`,
        "Boundary below",
        "Drag the boundary below",
      )}
      {grip(
        "w",
        `inset-y-3 -left-1 w-2 cursor-ew-resize ${EDGE_LINE} before:inset-y-0 before:left-[3px] before:w-0.5`,
        "Boundary left of",
        "Drag the boundary to the left",
      )}
      {grip(
        "e",
        `inset-y-3 -right-1 w-2 cursor-ew-resize ${EDGE_LINE} before:inset-y-0 before:left-[3px] before:w-0.5`,
        "Boundary right of",
        "Drag the boundary to the right",
      )}
      {grip("move", "sr-only", "Move", "Move: drop on another face to trade places")}
      {grip(
        "corner",
        "right-0 bottom-0 size-3.5 cursor-se-resize border-r-2 border-b-2 border-line-strong hover:border-accent",
        "Resize",
        "Resize",
      )}
    </>
  );
}

function area(cell: RackCell): React.CSSProperties {
  return {
    gridColumn: `${cell.x + 1} / span ${cell.w}`,
    gridRow: `${cell.y + 1} / span ${cell.h}`,
  };
}

function sameRack(a: RackLayout, b: RackLayout): boolean {
  return deepEqual(a.slots ?? [], b.slots ?? []);
}

function samePreview(a: Preview, b: Preview): boolean {
  return a.dropped === b.dropped && sameRack(a.layout, b.layout) && deepEqual(a.ghost, b.ghost);
}
