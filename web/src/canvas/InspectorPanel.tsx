import { X } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "../components/BaseControls";
import { ICON_BTN_SM_FAINT } from "../components/controls";
import { Icon } from "../components/Icon";
import { oneOf, orNull } from "../lib/remembered";
import { useNodeProblems } from "../lib/resolved";
import type { PatchNode } from "../lib/types";
import { useWorkspaceContext } from "./context";
import { isResizable, nodePorts } from "./graph";
import { InspectSection, NoInspect, useSlotSetters } from "./inspector";
import { NodeFace } from "./nodes";
import {
  BASEBAND_VIEWS,
  BasebandTabs,
  BasebandView,
  type BasebandView as View,
} from "./nodes/BasebandView";
import { PanelSurface } from "./nodes/NodeShell";
import { readPalette } from "./nodes/scopePalette";
import { inPopup, isTyping } from "./useHotkeys";
import { useNodeRemembered } from "./useNodeRemembered";

const IS_CHART = orNull(oneOf(BASEBAND_VIEWS));

export function InspectorPanel() {
  const workspace = useWorkspaceContext();
  const setters = useSlotSetters();
  const inspected = workspace.inspected;
  const inspect = workspace.inspect;
  const node = workspace.graph.nodes.find((candidate) => candidate.id === inspected) ?? null;
  const missing = inspected !== null && node === null;
  const [foot, setFoot] = useState<HTMLElement | null>(null);

  useEffect(() => {
    if (missing) {
      inspect(null);
    }
  }, [missing, inspect]);
  useEscapeCloses(node !== null, inspect);

  if (node === null || setters === null) {
    return null;
  }
  const { setHead, setBody } = setters;
  return (
    <aside
      aria-label="Inspector"
      className="flex w-96 shrink-0 flex-col border-l border-line bg-panel [--meter-label:4.5rem]"
    >
      <header className="flex h-10 shrink-0 items-center gap-2 border-b border-line pr-2 pl-4">
        <span ref={setHead} className="min-w-0 flex-1" />
        <Button
          type="button"
          aria-label="Close inspector"
          className={ICON_BTN_SM_FAINT}
          onClick={() => inspect(null)}
        >
          <Icon glyph={X} size={12} />
        </Button>
      </header>
      <FaceCopy node={node} foot={foot} />
      <div className="relative -mt-px min-h-0 flex-1 overflow-y-auto">
        <Problems node={node.id} />
        <div ref={setBody} className="flex flex-col" />
      </div>
      <div
        ref={setFoot}
        className="hidden shrink-0 border-t border-line bg-panel-2 has-[>:not(:empty)]:block"
      />
      <Baseband node={node} />
    </aside>
  );
}

function useEscapeCloses(open: boolean, inspect: (node: string | null) => void) {
  useEffect(() => {
    if (!open) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !isTyping(event.target) && !inPopup(event.target)) {
        inspect(null);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, inspect]);
}

/**
 * The node's own face, live, so what the canvas shows is here too. A display face, such as a
 * scope or a map, stays on the canvas: the panel has no room to spare for a second one.
 */
function FaceCopy({ node, foot }: { node: PatchNode; foot: HTMLElement | null }) {
  if (isResizable(node.kind)) {
    return null;
  }
  return (
    <div className="flex shrink-0 flex-col border-b border-line bg-panel-2">
      <PanelSurface foot={foot}>
        <NoInspect>
          <NodeFace key={node.id} node={node} />
        </NoInspect>
      </PanelSurface>
    </div>
  );
}

function Baseband({ node }: { node: PatchNode }) {
  const workspace = useWorkspaceContext();
  const [palette] = useState(readPalette);
  const [view, setView] = useNodeRemembered<View | null>(node.id, "basebandView", null, IS_CHART);
  const [big, setBig] = useState(false);
  useEscapeLeaves(big, () => setBig(false));
  const sends = nodePorts(workspace.context, node).some(
    (port) => port.direction === "out" && port.port_type === "baseband",
  );
  if (!sends) {
    return null;
  }
  if (view === null) {
    return (
      <div className="shrink-0 border-t border-line">
        <BasebandTabs onPick={setView} />
      </div>
    );
  }
  return (
    <div
      className={
        big
          ? "fixed inset-0 z-40 flex flex-col bg-bg"
          : "flex h-2/5 min-h-72 shrink-0 flex-col border-t border-line"
      }
    >
      <NoInspect>
        <BasebandView
          key={node.id}
          node={node.id}
          colormap={palette}
          view={view}
          onView={setView}
          expanded={big}
          onExpand={() => setBig(!big)}
          onClose={() => {
            setBig(false);
            setView(null);
          }}
        />
      </NoInspect>
    </div>
  );
}

/** Esc leaves a full-screen chart first, before it would close the panel. */
function useEscapeLeaves(big: boolean, leave: () => void) {
  useEffect(() => {
    if (!big) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || isTyping(event.target) || inPopup(event.target)) {
        return;
      }
      event.stopPropagation();
      leave();
    };
    window.addEventListener("keydown", onKeyDown, { capture: true });
    return () => window.removeEventListener("keydown", onKeyDown, { capture: true });
  }, [big, leave]);
}

function Problems({ node }: { node: string }) {
  const problems = useNodeProblems(node);
  if (problems.length === 0) {
    return null;
  }
  return (
    <InspectSection label="Problems">
      <ul className="flex flex-col gap-1 text-warn text-xs">
        {problems.map((problem, index) => (
          <li key={`${index}:${problem.text}`}>{problem.text}</li>
        ))}
      </ul>
    </InspectSection>
  );
}
