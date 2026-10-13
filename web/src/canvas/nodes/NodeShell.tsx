import { Handle, NodeResizer, Position, useUpdateNodeInternals } from "@xyflow/react";
import { CircleAlert, Maximize2, Minimize2, Pin, X } from "lucide-react";
import {
  createContext,
  type ReactNode,
  type RefObject,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { Button } from "../../components/BaseControls";
import { ICON_BTN_SM } from "../../components/controls";
import { FaceAlert } from "../../components/FaceAlert";
import { Icon } from "../../components/Icon";
import { PortalContainerProvider } from "../../components/PortalContainer";
import { activityText, idlePort, usePortActivity } from "../../lib/activity";
import { kindLabel } from "../../lib/eventKinds";
import { ignoredOf, useNodeProblems } from "../../lib/resolved";
import type { NodeCategory, PatchNode, PortSpec, PortType } from "../../lib/types";
import { portKey, useWiringStore } from "../../lib/wiring";
import { HUE_TEXT, portHue } from "../../sinks/hues";
import { useWorkspaceContext } from "../context";
import {
  echoesWire,
  fitWidth,
  handleSignature,
  isPinned,
  isResizable,
  nodeMinSize,
  nodePorts,
  PORT_STEP_PX,
  PORT_TOP_PX,
  pin,
  portLabel,
  unpin,
} from "../graph";
import { dropNodes } from "../remove";
import { movesCanvas, wheelStaysOnFace } from "../wheel";
import { NodeName } from "./NodeName";
import { offsetWithin } from "./portAnchor";

const Surface = createContext<"canvas" | "rack">("rack");

export function CanvasSurface({ children }: { children: ReactNode }) {
  return <Surface value="canvas">{children}</Surface>;
}

const Active = createContext(true);

export function useFaceActive(): boolean {
  return useContext(Active);
}

export type WheelClaim = (event: WheelEvent) => boolean;

type WheelHold = (claim: WheelClaim | null) => void;

const WheelClaimSlot = createContext<WheelHold | null>(null);

export function useFaceWheel(claim: WheelClaim): void {
  const hold = useContext(WheelClaimSlot);
  useEffect(() => {
    if (hold === null) {
      return;
    }
    hold(claim);
    return () => hold(null);
  }, [hold, claim]);
}

type Anchors = Readonly<Record<string, number>>;

interface AnchorSlot {
  container: RefObject<HTMLDivElement | null>;
  place: (port: string, top: number | null) => void;
}

const PortAnchors = createContext<AnchorSlot | null>(null);

function placed(anchors: Anchors, port: string, top: number | null): Anchors {
  if (top === null) {
    const { [port]: _dropped, ...rest } = anchors;
    return port in anchors ? rest : anchors;
  }
  return anchors[port] === top ? anchors : { ...anchors, [port]: top };
}

export function PortAnchor({ port }: { port: string }) {
  const slot = useContext(PortAnchors);
  const mark = useRef<HTMLSpanElement>(null);
  useLayoutEffect(() => {
    const host = slot?.container.current ?? null;
    const element = mark.current;
    if (slot === null || host === null || element === null) {
      return;
    }
    const measure = () => slot.place(port, offsetWithin(element, host));
    measure();
    if (typeof ResizeObserver === "undefined") {
      return () => slot.place(port, null);
    }
    const observer = new ResizeObserver(measure);
    observer.observe(host);
    if (element.parentElement !== null) {
      observer.observe(element.parentElement);
    }
    return () => {
      observer.disconnect();
      slot.place(port, null);
    };
  }, [slot, port]);
  return <span ref={mark} aria-hidden className="w-0 shrink-0 self-stretch" />;
}

const CATEGORY_STRIP: Record<NodeCategory, string> = {
  source: "bg-cat-source shadow-[0_0_8px_var(--color-cat-source)]",
  channel: "bg-cat-channel shadow-[0_0_8px_var(--color-cat-channel)]",
  tool: "bg-cat-tool shadow-[0_0_8px_var(--color-cat-tool)]",
  output: "bg-cat-output shadow-[0_0_8px_var(--color-cat-output)]",
};

const CATEGORY_BAR: Record<NodeCategory, string> = {
  source: "bg-cat-source/17 shadow-[inset_0_2px_0_var(--color-cat-source)]",
  channel: "bg-cat-channel/17 shadow-[inset_0_2px_0_var(--color-cat-channel)]",
  tool: "bg-cat-tool/17 shadow-[inset_0_2px_0_var(--color-cat-tool)]",
  output: "bg-cat-output/17 shadow-[inset_0_2px_0_var(--color-cat-output)]",
};

function PortGlyph({ type }: { type: PortType }) {
  const common = {
    fill: "currentColor",
    stroke: "var(--color-line-strong)",
    strokeWidth: 1,
  };
  return (
    <svg aria-hidden viewBox="0 0 12 12" className="pointer-events-none size-3 overflow-visible">
      {type === "array" ? (
        <>
          <circle cx="6" cy="6" r="4.5" {...common} />
          <circle cx="6" cy="6" r="2.2" fill="none" stroke="var(--color-panel)" strokeWidth={1} />
        </>
      ) : type === "iq" || type === "position" ? (
        <circle cx="6" cy="6" r="4.5" {...common} />
      ) : type === "bearing" ? (
        <path d="M6 1 9.5 11 6 8.5 2.5 11Z" {...common} />
      ) : type === "baseband" ? (
        <path d="M10.5 6 A4.5 4.5 0 0 1 1.5 6 Z" {...common} />
      ) : type === "audio" ? (
        <path d="M6 1 11 6 6 11 1 6Z" {...common} />
      ) : type === "events" ? (
        <path d="M3 1h6l2 5-2 5H3L1 6Z" {...common} />
      ) : type === "video" ? (
        <path d="M6 1 11 11H1Z" {...common} />
      ) : (
        <path d="M1 1 11 6 1 11Z" {...common} />
      )}
    </svg>
  );
}

export interface NodeShellProps {
  node: PatchNode;
  title: string;
  category: NodeCategory;
  subtitle?: ReactNode;
  badge?: ReactNode;
  actions?: ReactNode;
  width?: number;
  children: ReactNode;
}

export function NodeShell({
  node,
  title,
  category,
  subtitle,
  badge,
  actions,
  width,
  children,
}: NodeShellProps) {
  const workspace = useWorkspaceContext();
  const surface = useContext(Surface);
  const remove = useRemoveNode(node);
  const ports = surface === "canvas" ? nodePorts(workspace.context, node) : [];
  const pinned = isPinned(workspace.rack, node.id);
  const selected = workspace.selected === node.id;
  const active = surface === "rack" || selected;
  const minimum = nodeMinSize(node.kind, ports);
  const portalContainer = useRef<HTMLDivElement>(null);
  const wheelClaim = useRef<WheelClaim | null>(null);
  const holdWheel = useCallback<WheelHold>((claim) => {
    wheelClaim.current = claim;
  }, []);
  const full = workspace.expanded === node.id;
  const [anchors, setAnchors] = useState<Anchors>({});
  const place = useCallback(
    (port: string, top: number | null) => setAnchors((current) => placed(current, port, top)),
    [],
  );
  const anchorSlot = useMemo<AnchorSlot>(() => ({ container: portalContainer, place }), [place]);
  useWheelRouting(portalContainer, wheelClaim);
  useHandleRefresh(node.id, ports, anchors, surface === "canvas");

  return (
    <div
      ref={portalContainer}
      style={surface === "canvas" ? canvasSize(node.kind, minimum.h, width) : undefined}
      className={`relative flex h-full min-h-0 flex-col ${surface === "canvas" && width === undefined && fitWidth(node.kind) !== null ? "w-max" : "w-full"} rounded-control border bg-linear-to-b from-panel-3 to-panel shadow-node ${
        selected ? "border-accent" : "border-line"
      }`}
    >
      <PortalContainerProvider container={portalContainer}>
        {surface === "canvas" && isResizable(node.kind) && (
          <NodeResizer
            minWidth={minimum.w}
            minHeight={minimum.h}
            autoScale={false}
            lineClassName="!border-accent/40"
            handleClassName="!size-2 !rounded-inset !border-accent !bg-panel"
          />
        )}
        <header
          className={`flex h-6.5 shrink-0 items-center gap-2 rounded-t-control border-b border-line pr-1 pl-2 ${CATEGORY_BAR[category]} ${
            surface === "canvas" ? "node-drag cursor-grab active:cursor-grabbing" : ""
          }`}
        >
          <span
            aria-hidden
            className={`size-[7px] shrink-0 rounded-full ${CATEGORY_STRIP[category]}`}
          />
          <NodeName node={node} title={title} />
          {badge !== undefined && (
            <span className="shrink-0 font-mono text-2xs text-ink-faint">{badge}</span>
          )}
          <ProblemMark node={node.id} />
          {subtitle !== undefined && (
            <span className="ml-auto truncate font-mono text-2xs text-ink-faint">{subtitle}</span>
          )}
          <span
            className={`nodrag flex cursor-auto items-center gap-0.5 ${subtitle === undefined ? "ml-auto" : ""}`}
          >
            {actions}
            <Button
              type="button"
              aria-label={full ? "Leave full screen" : "Show full screen"}
              aria-pressed={full}
              title={full ? "Back to its usual size" : "Fill the window with this face"}
              className={`${ICON_BTN_SM} ${full ? "bg-accent/15 text-accent" : "text-ink-faint"}`}
              onClick={() => workspace.expand(full ? null : node.id)}
            >
              <Icon glyph={full ? Minimize2 : Maximize2} size={12} />
            </Button>
            <Button
              type="button"
              aria-label={pinned ? "Unpin from the rack" : "Pin to the rack"}
              aria-pressed={pinned}
              title={pinned ? "On the rack: click to take it off" : "Pin to the rack"}
              className={`${ICON_BTN_SM} ${pinned ? "bg-accent/15 text-accent" : "text-ink-faint"}`}
              onClick={() =>
                workspace.edit((snapshot) => ({
                  ...snapshot,
                  rack: pinned
                    ? unpin(snapshot.rack ?? {}, node.id)
                    : pin(snapshot.rack ?? {}, node.id),
                }))
              }
            >
              <Icon glyph={Pin} size={12} filled={pinned} />
            </Button>
            <Button
              type="button"
              aria-label={`Remove ${node.label ?? title}`}
              title="Remove from the patch"
              className={`${ICON_BTN_SM} text-ink-faint hover:text-danger`}
              onClick={remove}
            >
              <Icon glyph={X} size={12} />
            </Button>
          </span>
        </header>

        <FaceAlert node={node.id} />

        <div
          className="relative flex min-h-0 flex-1 flex-col overflow-hidden rounded-b-control nodrag nopan"
          onPointerDownCapture={surface === "canvas" ? () => workspace.select(node.id) : undefined}
        >
          <Active value={active}>
            <WheelClaimSlot value={holdWheel}>
              <PortAnchors value={anchorSlot}>{children}</PortAnchors>
            </WheelClaimSlot>
          </Active>
        </div>

        {ports.map((port, index) => (
          <PortHandle
            key={`${port.direction}:${port.name}`}
            node={node.id}
            port={port}
            color={HUE_TEXT[portHue(workspace.context.catalog, port.port_type)]}
            label={portLabel(port.name, ports)}
            offset={anchors[port.name] ?? PORT_TOP_PX + PORT_STEP_PX * indexOnSide(ports, index)}
            labelled={!(port.name in anchors) && !echoesWire(workspace.graph, node.id, port)}
          />
        ))}
      </PortalContainerProvider>
    </div>
  );
}

function ProblemMark({ node }: { node: string }) {
  const problems = useNodeProblems(node);
  if (problems.length === 0) {
    return null;
  }
  const text = problems.map((problem) => problem.text).join("\n");
  return (
    <span role="img" aria-label={text} title={text} className="flex shrink-0 text-warn">
      <Icon glyph={CircleAlert} size={12} />
    </span>
  );
}

function canvasSize(
  kind: PatchNode["kind"],
  minHeight: number,
  width: number | undefined,
): React.CSSProperties {
  return width === undefined ? { minHeight, ...fitWidth(kind) } : { minHeight, width };
}

function useWheelRouting(
  face: RefObject<HTMLDivElement | null>,
  claim: RefObject<WheelClaim | null>,
): void {
  useEffect(() => {
    const host = face.current;
    if (host === null) {
      return;
    }
    const onWheel = (event: WheelEvent) => {
      if (movesCanvas(event)) {
        return;
      }
      if (wheelStaysOnFace(event, host)) {
        event.stopPropagation();
        return;
      }
      if (claim.current?.(event) === true) {
        event.preventDefault();
        event.stopPropagation();
      }
    };
    host.addEventListener("wheel", onWheel, { passive: false });
    return () => host.removeEventListener("wheel", onWheel);
  }, [face, claim]);
}

function useHandleRefresh(
  id: string,
  ports: readonly PortSpec[],
  anchors: Anchors,
  onCanvas: boolean,
): void {
  const updateNodeInternals = useUpdateNodeInternals();
  const handles = handleSignature(ports);
  const placement = Object.entries(anchors)
    .map(([port, top]) => `${port}@${Math.round(top)}`)
    .join(",");
  useEffect(() => {
    if (onCanvas && (handles !== "" || placement !== "")) {
      updateNodeInternals(id);
    }
  }, [id, handles, placement, onCanvas, updateNodeInternals]);
}

function useRemoveNode(node: PatchNode): () => void {
  const workspace = useWorkspaceContext();
  return () => dropNodes(workspace, [node.id]);
}

function indexOnSide(ports: readonly PortSpec[], index: number): number {
  const side = ports[index]?.direction;
  return ports.slice(0, index).filter((port) => port.direction === side).length;
}

function PortHandle({
  node,
  port,
  color,
  label,
  offset,
  labelled,
}: {
  node: string;
  port: PortSpec;
  color: string;
  label: string;
  offset: number;
  labelled: boolean;
}) {
  const out = port.direction === "out";
  const refusal = useWiringStore((state) =>
    out ? undefined : state.verdicts?.get(portKey({ node, port: port.name })),
  );
  const workspace = useWorkspaceContext();
  const ignored = ignoredOf(workspace.resolved, node, port.name).map((kind) =>
    kindLabel(kind, workspace.context.catalog.event_kinds),
  );
  const activity = usePortActivity(node, port.name);
  const named = port.note == null ? `${label} (${port.port_type})` : `${label}: ${port.note}`;
  const description = [
    named,
    ignored.length === 0 ? null : `Ignores ${ignored.join(", ")}`,
    activityText(activity),
  ]
    .filter((line) => line !== null)
    .join("\n");
  return (
    <>
      <Handle
        id={port.name}
        type={out ? "source" : "target"}
        position={out ? Position.Right : Position.Left}
        style={{ top: offset }}
        title={refusal ?? description}
        aria-label={`${out ? "output" : "input"} ${description}`}
        className={`!size-3 !border-0 !bg-transparent ${color} ${idlePort(activity) ? "opacity-40" : ""}`}
      >
        <PortGlyph type={port.port_type} />
      </Handle>
      {labelled && (
        <span
          aria-hidden
          style={{ top: offset }}
          className={`pointer-events-none absolute z-10 -translate-y-1/2 rounded-control bg-bg/85 px-1 font-mono text-3xs whitespace-nowrap select-none text-ink-faint ${
            out ? "left-full ml-2.5" : "right-full mr-2.5"
          }`}
        >
          {label}
        </span>
      )}
    </>
  );
}

export function FaceBody({
  children,
  scroll = true,
  title,
}: {
  children: ReactNode;
  scroll?: boolean;
  title?: string;
}) {
  return (
    <div
      title={title}
      className={`flex min-h-0 flex-1 flex-col overflow-x-hidden ${scroll ? "overflow-y-auto" : ""}`}
    >
      {children}
    </div>
  );
}

export function FaceEmpty({ hint }: { hint?: string }) {
  return (
    <div className="flex min-h-12 flex-1 items-center justify-center p-3">
      {hint !== undefined && (
        <span className="text-center text-xs text-balance text-ink-faint">{hint}</span>
      )}
    </div>
  );
}

export function FaceFooter({ children }: { children: ReactNode }) {
  return (
    <div className="flex shrink-0 flex-wrap items-center justify-end gap-2 border-t border-line px-2.5 py-2">
      {children}
    </div>
  );
}
