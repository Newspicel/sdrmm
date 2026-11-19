import { Handle, NodeResizer, Position, useStoreApi } from "@xyflow/react";
import { CircleAlert, Maximize2, Minimize2, Pin, SlidersHorizontal, X } from "lucide-react";
import {
  createContext,
  type ReactNode,
  type RefObject,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
} from "react";
import { createPortal } from "react-dom";
import { Button } from "../../components/BaseControls";
import { ICON_BTN_SM } from "../../components/controls";
import { FaceAlert } from "../../components/FaceAlert";
import { FaceFault } from "../../components/face/Fault";
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
  handleSignature,
  isPinned,
  isResizable,
  nodeMinSize,
  nodePorts,
  nodeSize,
  PORT_STEP_PX,
  PORT_TOP_PX,
  pin,
  portLabel,
  unpin,
} from "../graph";
import { Inspect } from "../inspector";
import { dropNodes } from "../remove";
import { movesCanvas, wheelStaysOnFace } from "../wheel";
import { NodeName } from "./NodeName";
import type { Anchors } from "./portAnchor";
import { type AnchorSlot, useContentHeight, useFaceLayout } from "./useFaceLayout";

const Surface = createContext<"canvas" | "rack" | "panel">("rack");

export function CanvasSurface({ children }: { children: ReactNode }) {
  return <Surface value="canvas">{children}</Surface>;
}

const PanelFoot = createContext<HTMLElement | null>(null);

/**
 * A face drawn bare in the inspector, without its frame, header or ports. Its footer actions
 * go to `foot`, below the settings.
 */
export function PanelSurface({
  foot,
  children,
}: {
  foot: HTMLElement | null;
  children: ReactNode;
}) {
  return (
    <Surface value="panel">
      <PanelFoot value={foot}>{children}</PanelFoot>
    </Surface>
  );
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

const PortAnchors = createContext<AnchorSlot | null>(null);

export function PortAnchor({ port }: { port: string }) {
  const slot = useContext(PortAnchors);
  const mark = useRef<HTMLSpanElement>(null);
  useLayoutEffect(() => {
    const element = mark.current;
    return slot === null || element === null ? undefined : slot.track(port, element);
  }, [slot, port]);
  return <span ref={mark} aria-hidden className="w-0 shrink-0 self-stretch" />;
}

const CATEGORY_STRIP: Record<NodeCategory, string> = {
  source: "bg-cat-source shadow-[0_0_8px_var(--color-cat-source)]",
  channel: "bg-cat-channel shadow-[0_0_8px_var(--color-cat-channel)]",
  encoder: "bg-cat-encoder shadow-[0_0_8px_var(--color-cat-encoder)]",
  tool: "bg-cat-tool shadow-[0_0_8px_var(--color-cat-tool)]",
  output: "bg-cat-output shadow-[0_0_8px_var(--color-cat-output)]",
};

/** What a live node is doing, lit on its header dot: the one place a node's state changes colour. */
export interface LiveSignal {
  tone: "ok" | "warn" | "danger";
  label: string;
}

const PASSING: LiveSignal = { tone: "ok", label: "Passing audio" };

const LIVE_STRIP: Record<LiveSignal["tone"], string> = {
  ok: "bg-ok shadow-[0_0_10px_2px_var(--color-ok)]",
  warn: "bg-warn shadow-[0_0_10px_2px_var(--color-warn)]",
  danger: "bg-danger shadow-[0_0_10px_2px_var(--color-danger)]",
};

function liveSignal(live: boolean | LiveSignal | undefined): LiveSignal | null {
  if (live === true) {
    return PASSING;
  }
  return live === false || live === undefined ? null : live;
}

const CATEGORY_TINT: Record<NodeCategory, string> = {
  source: "bg-cat-source/15",
  channel: "bg-cat-channel/15",
  encoder: "bg-cat-encoder/15",
  tool: "bg-cat-tool/15",
  output: "bg-cat-output/15",
};

const CATEGORY_RULE: Record<NodeCategory, string> = {
  source: "shadow-[inset_0_2px_0_var(--color-cat-source)]",
  channel: "shadow-[inset_0_2px_0_var(--color-cat-channel)]",
  encoder: "shadow-[inset_0_2px_0_var(--color-cat-encoder)]",
  tool: "shadow-[inset_0_2px_0_var(--color-cat-tool)]",
  output: "shadow-[inset_0_2px_0_var(--color-cat-output)]",
};

const SELECTED_RULE = "shadow-[inset_0_2px_0_var(--color-accent)]";

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
  live?: boolean | LiveSignal;
  width?: number;
  rowPorts?: readonly string[];
  children: ReactNode;
}

export function NodeShell(props: NodeShellProps) {
  return useContext(Surface) === "panel" ? (
    <BareShell>{props.children}</BareShell>
  ) : (
    <FramedShell {...props} />
  );
}

function BareShell({ children }: { children: ReactNode }) {
  const container = useRef<HTMLDivElement>(null);
  return (
    <div ref={container} className="flex min-h-0 flex-1 flex-col">
      <PortalContainerProvider container={container}>{children}</PortalContainerProvider>
    </div>
  );
}

function FramedShell({
  node,
  title,
  category,
  subtitle,
  badge,
  actions,
  live = false,
  width,
  rowPorts,
  children,
}: NodeShellProps) {
  const workspace = useWorkspaceContext();
  const surface = useContext(Surface);
  const signal = liveSignal(live);
  const remove = useRemoveNode(node);
  const ports = surface === "canvas" ? nodePorts(workspace.context, node) : [];
  const pinned = isPinned(workspace.rack, node.id);
  const selected = workspace.selected === node.id;
  const active = surface === "rack" || selected;
  const minimum = nodeMinSize(node, ports);
  const wheelClaim = useRef<WheelClaim | null>(null);
  const holdWheel = useCallback<WheelHold>((claim) => {
    wheelClaim.current = claim;
  }, []);
  const full = workspace.expanded === node.id;
  const inspected = workspace.inspected === node.id;
  const { container: portalContainer, anchors, slot: anchorSlot } = useFaceLayout();
  useWheelRouting(portalContainer, wheelClaim);
  useHandleRefresh(node.id, ports, anchors, portalContainer, surface === "canvas");

  return (
    <div
      ref={portalContainer}
      style={surface === "canvas" ? canvasSize(node, minimum.h, width) : undefined}
      className={`relative flex h-full min-h-0 w-full flex-col rounded-control border bg-linear-to-b from-panel-3 to-panel shadow-node ${
        selected ? "border-accent" : "border-line"
      }`}
    >
      <PortalContainerProvider container={portalContainer}>
        {surface === "canvas" && isResizable(node) && (
          <NodeResizer
            minWidth={minimum.w}
            minHeight={minimum.h}
            autoScale={false}
            lineClassName="!border-accent/40"
            handleClassName="!size-2 !rounded-inset !border-accent !bg-panel"
          />
        )}
        <Inspect node={node.id} slot="head">
          <PanelHead
            name={node.label ?? title}
            category={category}
            live={live}
            badge={badge}
            subtitle={subtitle}
          />
        </Inspect>
        <header
          className={`flex h-6.5 shrink-0 items-center gap-2 rounded-t-control border-b border-line pr-1 pl-2 transition-colors duration-150 ${
            CATEGORY_TINT[category]
          } ${selected ? SELECTED_RULE : CATEGORY_RULE[category]} ${
            surface === "canvas" ? "node-drag cursor-grab active:cursor-grabbing" : ""
          }`}
        >
          {signal !== null ? (
            <span
              role="img"
              aria-label={signal.label}
              title={signal.label}
              className={`size-[7px] shrink-0 rounded-full ${LIVE_STRIP[signal.tone]}`}
            />
          ) : (
            <span
              aria-hidden
              className={`size-[7px] shrink-0 rounded-full ${CATEGORY_STRIP[category]}`}
            />
          )}
          <NodeName node={node} title={title} />
          {badge !== undefined && (
            <span className="shrink-0 font-mono text-2xs text-ink-dim">{badge}</span>
          )}
          <ProblemMark node={node.id} />
          <span className="nodrag flex cursor-auto">
            <FaceAlert node={node.id} />
          </span>
          {subtitle !== undefined && (
            <span className="flex min-w-0 grow basis-0 flex-wrap items-center justify-end self-stretch overflow-hidden font-mono text-2xs whitespace-nowrap text-ink-dim">
              <span aria-hidden className="h-full w-0" />
              {subtitle}
            </span>
          )}
          <span
            className={`nodrag flex cursor-auto items-center gap-0.5 ${subtitle === undefined ? "ml-auto" : ""}`}
            onClick={(event) => event.stopPropagation()}
          >
            {actions}
            <Button
              type="button"
              aria-label={inspected ? "Close inspector" : "Inspect"}
              aria-pressed={inspected}
              title="Settings and status"
              className={`${ICON_BTN_SM} ${inspected ? "bg-accent/15 text-accent" : "text-ink-faint"}`}
              onClick={() => workspace.inspect(inspected ? null : node.id)}
            >
              <Icon glyph={SlidersHorizontal} size={12} />
            </Button>
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
            waiting={rowPorts?.includes(port.name) === true && !(port.name in anchors)}
          />
        ))}
      </PortalContainerProvider>
    </div>
  );
}

function PanelHead({
  name,
  category,
  live,
  badge,
  subtitle,
}: {
  name: string;
  category: NodeCategory;
  live: boolean | LiveSignal;
  badge: ReactNode;
  subtitle: ReactNode;
}) {
  const signal = liveSignal(live);
  return (
    <span className="flex min-w-0 items-center gap-2">
      <span
        aria-hidden
        className={`size-2 shrink-0 rounded-full ${signal === null ? CATEGORY_STRIP[category] : LIVE_STRIP[signal.tone]}`}
      />
      <span className="truncate font-semibold text-sm text-ink">{name}</span>
      {badge !== undefined && (
        <span className="shrink-0 font-mono text-2xs text-ink-dim">{badge}</span>
      )}
      {subtitle !== undefined && (
        <span className="ml-auto min-w-0 shrink-[4] truncate font-mono text-2xs text-ink-dim">
          {subtitle}
        </span>
      )}
    </span>
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
  node: PatchNode,
  minHeight: number,
  width: number | undefined,
): React.CSSProperties {
  if (width !== undefined) {
    return { minHeight, width };
  }
  return isResizable(node) ? { minHeight } : { minHeight, width: nodeSize(node).w };
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
  container: RefObject<HTMLDivElement | null>,
  onCanvas: boolean,
): void {
  const store = useStoreApi();
  const handles = handleSignature(ports);
  const placement = Object.entries(anchors)
    .map(([port, top]) => `${port}@${Math.round(top)}`)
    .join(",");
  useLayoutEffect(() => {
    const nodeElement = container.current?.closest<HTMLDivElement>(".react-flow__node");
    if (!onCanvas || nodeElement == null || (handles === "" && placement === "")) {
      return;
    }
    store.getState().updateNodeInternals(new Map([[id, { id, nodeElement, force: true }]]), {
      triggerFitView: false,
    });
  }, [id, handles, placement, container, onCanvas, store]);
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
  waiting,
}: {
  node: string;
  port: PortSpec;
  color: string;
  label: string;
  offset: number;
  labelled: boolean;
  waiting: boolean;
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
        className={`!size-3 !border-0 !bg-transparent transition-opacity duration-150 ${color} ${
          waiting ? "pointer-events-none opacity-0" : idlePort(activity) ? "opacity-45" : ""
        }`}
      >
        <PortGlyph type={port.port_type} />
      </Handle>
      {labelled && !waiting && (
        <span
          aria-hidden
          style={{ top: offset }}
          className={`pointer-events-none absolute z-10 -translate-y-1/2 rounded-control bg-bg/85 px-1 font-mono text-2xs whitespace-nowrap select-none text-ink-faint ${
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

/**
 * Why a node idles: a server fault, then what the server waits for. Only the fault is red.
 * An unwired node says nothing; its empty port already shows why.
 */
export function FaceStanding({
  unwired,
  error,
  waiting,
}: {
  unwired: boolean;
  error?: string | null;
  waiting?: string | null;
}) {
  if (unwired) {
    return null;
  }
  if (error != null) {
    return <FaceFault message={error} />;
  }
  return waiting == null ? null : <FaceEmpty hint={waiting} />;
}

export function FaceFooter({ children }: { children: ReactNode }) {
  const panel = useContext(Surface) === "panel";
  const foot = useContext(PanelFoot);
  if (panel) {
    return foot === null
      ? null
      : createPortal(
          <div className="flex flex-wrap items-center justify-end gap-2 px-4 py-2.5 empty:hidden">
            {children}
          </div>,
          foot,
        );
  }
  return <FramedFooter>{children}</FramedFooter>;
}

function FramedFooter({ children }: { children: ReactNode }) {
  const inner = useRef<HTMLDivElement>(null);
  const height = useContentHeight(inner);
  return (
    <div
      className="ease-height shrink-0 overflow-hidden bg-ink/5"
      style={height === null ? undefined : { height }}
    >
      <div
        ref={inner}
        className="flex flex-wrap items-center justify-end gap-2 px-2.5 py-2 empty:hidden"
      >
        {children}
      </div>
    </div>
  );
}
