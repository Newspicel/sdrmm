import {
  Background,
  BackgroundVariant,
  type Node,
  ReactFlow,
  useEdgesState,
  useNodesInitialized,
  useNodesState,
  useReactFlow,
  type Viewport,
} from "@xyflow/react";
import {
  type ReactNode,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { Button } from "../components/BaseControls";
import { BTN_QUIET, BTN_QUIET_DANGER, SURFACE } from "../components/controls";
import { CANVAS_MOVED } from "../components/Popover";
import { zoomSettled } from "../gl/drawLoop";
import { useUiScale } from "../lib/prefs";
import { createPointerSender, draggedByOthers, othersOf, usePresenceStore } from "../lib/presence";
import {
  isFiniteNumber,
  isRecord,
  orNull,
  readRemembered,
  rememberedKey,
  storeRemembered,
  within,
} from "../lib/remembered";
import { useCanEdit } from "../lib/role";
import type { ClientCommand, PatchGraph, PatchNode, Pointer } from "../lib/types";
import { CanvasPalette, type ScreenPoint } from "./CanvasPalette";
import { useClipboard } from "./clipboard";
import { useWorkspaceContext } from "./context";
import { toFlowEdges } from "./flowEdges";
import { movedGeometry, withGeometry } from "./geometry";
import {
  isPinned,
  isResizable,
  naturalSize,
  nodeOf,
  patchNode,
  pin,
  removeEdge,
  sameGraph,
  unpin,
} from "./graph";
import { useGridStep } from "./grid";
import { useConnections, useGraphChanges } from "./handlers";
import { type MenuPlace, menuPlace } from "./menuPlace";
import { flowTypeOf, NODE_TYPES } from "./nodes";
import { ReplaceDecoder } from "./nodes/ReplaceDecoder";
import { PeerLayer } from "./PeerLayer";
import { focusNode } from "./selection";
import { isTyping } from "./useHotkeys";

export interface FlowData extends Record<string, unknown> {
  node: PatchNode;
}

const FIT_VIEW = { padding: 0.12, maxZoom: 1 } as const;
const MIN_ZOOM = 0.15;
const MAX_ZOOM = 2;
const IS_ZOOM = within(MIN_ZOOM, MAX_ZOOM);
const IS_SAVED_VIEWPORT = orNull(isViewport);

export function isViewport(value: unknown): value is Viewport {
  return (
    isRecord(value) && isFiniteNumber(value.x) && isFiniteNumber(value.y) && IS_ZOOM(value.zoom)
  );
}

const DELETE_KEYS = ["Backspace", "Delete"];

export function Canvas() {
  const workspace = useWorkspaceContext();
  const grid = useGridStep();
  const scale = useUiScale();
  const [nodes, setNodes, onNodesChange] = useNodesState<Node<FlowData>>(
    toFlowNodes(workspace.graph, scale),
  );
  const [edges, setEdges, onEdgesChange] = useEdgesState(
    toFlowEdges(workspace.graph, workspace.context),
  );

  const pasted = useRef<ReadonlySet<string>>(new Set());
  const selection = nodes.filter((node) => node.selected).map((node) => node.id);
  useClipboard(
    workspace,
    selection.length > 0 ? selection : workspace.selected === null ? [] : [workspace.selected],
    useCallback((ids: readonly string[]) => {
      pasted.current = new Set(ids);
    }, []),
  );

  const held = useRef<PatchGraph>(workspace.graph);
  const heldScale = useRef(scale);
  const context = workspace.context;
  useEffect(() => {
    if (sameGraph(held.current, workspace.graph) && heldScale.current === scale) {
      setEdges(toFlowEdges(workspace.graph, context));
      return;
    }
    held.current = workspace.graph;
    heldScale.current = scale;
    const fresh = pasted.current;
    const arrived = workspace.graph.nodes.some((node) => fresh.has(node.id));
    if (arrived) {
      pasted.current = new Set();
    }
    setNodes((previous) =>
      toFlowNodes(workspace.graph, scale).map((node) => {
        const mounted = previous.find((candidate) => candidate.id === node.id);
        const selected = arrived ? fresh.has(node.id) : (mounted?.selected ?? false);
        if (mounted === undefined) {
          return { ...node, selected };
        }
        const position = mounted.dragging === true ? mounted.position : node.position;
        return { ...mounted, ...node, position, selected };
      }),
    );
    setEdges(toFlowEdges(workspace.graph, context));
  }, [workspace.graph, context, scale, setNodes, setEdges]);

  const focus = workspace.selected;
  useEffect(() => {
    setNodes((previous) => focusNode(previous, focus));
    // oxlint-disable-next-line react/exhaustive-effect-dependencies -- a redrawn graph loses the focus ring
  }, [focus, workspace.graph, setNodes]);

  const flowRef = useRef<typeof nodes>([]);
  useLayoutEffect(() => {
    flowRef.current = nodes;
  });

  const edit = workspace.edit;
  const commitGeometry = useCallback(() => {
    const moved = movedGeometry(flowRef.current, held.current, heldScale.current);
    if (moved.size === 0) {
      return;
    }
    edit((snapshot) => ({ ...snapshot, graph: withGeometry(snapshot.graph, moved) }));
  }, [edit]);

  const publish = usePointer(workspace.socket);
  usePublishSelection(publish, selection.length > 0 ? selection : [workspace.selected ?? ""]);
  usePeerDrags(setNodes);

  const { handleNodesChange, handleEdgesChange } = useGraphChanges(
    workspace,
    onNodesChange,
    onEdgesChange,
    commitGeometry,
  );

  const { isValidConnection, onConnect, onConnectStart, onConnectEnd } = useConnections(workspace);
  const viewportKey = rememberedKey("viewport", workspace.id);
  const [saved] = useState(() => readRemembered(viewportKey, null, IS_SAVED_VIEWPORT));
  useFitOnceMeasured(saved === null);
  const { screenToFlowPosition } = useReactFlow();

  const [menu, setMenu] = useState<Menu | null>(null);
  const [replacing, setReplacing] = useState<string | null>(null);
  const [adding, setAdding] = useState<ScreenPoint | null>(null);
  const openMenu = useCallback((event: React.MouseEvent, target: Menu["target"]) => {
    event.preventDefault();
    setMenu({ x: event.clientX, y: event.clientY, target });
  }, []);
  const replaced = replacing === null ? undefined : nodeOf(workspace.graph, replacing);

  const canEdit = useCanEdit();
  const select = workspace.select;
  const claimed = menu !== null || adding !== null || workspace.expanded !== null;
  useEscapeDeselects(claimed, select, setNodes);

  return (
    <div
      className="relative flex min-h-0 flex-1 flex-col"
      onPointerMove={(event) => {
        if (event.target instanceof Element && event.currentTarget.contains(event.target)) {
          publish({ at: screenToFlowPosition({ x: event.clientX, y: event.clientY }) });
        }
      }}
      onPointerLeave={() => publish({ at: null })}
      onDoubleClick={(event) => {
        if (
          canEdit &&
          event.target instanceof Element &&
          event.target.classList.contains("react-flow__pane")
        ) {
          setAdding({ x: event.clientX, y: event.clientY });
        }
      }}
    >
      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={NODE_TYPES}
        onNodesChange={handleNodesChange}
        onEdgesChange={handleEdgesChange}
        onNodeDrag={(_event, _node, dragged) =>
          publish({
            dragging: dragged.map((node) => ({ node: node.id, position: node.position })),
          })
        }
        onNodeDragStop={() => {
          publish({ dragging: [] });
          commitGeometry();
        }}
        nodesDraggable={canEdit}
        nodesConnectable={canEdit}
        onConnect={onConnect}
        onConnectStart={onConnectStart}
        onConnectEnd={onConnectEnd}
        isValidConnection={isValidConnection}
        onPaneClick={() => {
          workspace.select(null);
          workspace.inspect(null);
          setMenu(null);
        }}
        onNodeClick={(event, node) => {
          setMenu(null);
          workspace.inspect(addsToSelection(event) ? null : node.id);
        }}
        onMoveStart={(event) => {
          if (event !== null) {
            window.dispatchEvent(new Event(CANVAS_MOVED));
          }
        }}
        onMoveEnd={(_event, viewport) => {
          zoomSettled(viewport.zoom);
          storeRemembered(viewportKey, viewport);
        }}
        onNodeContextMenu={
          canEdit ? (event, node) => openMenu(event, { kind: "node", id: node.id }) : undefined
        }
        onEdgeContextMenu={
          canEdit ? (event, edge) => openMenu(event, { kind: "edge", id: edge.id }) : undefined
        }
        onPaneContextMenu={
          canEdit ? (event) => openMenu(event as React.MouseEvent, { kind: "pane" }) : undefined
        }
        deleteKeyCode={canEdit && workspace.expanded === null ? DELETE_KEYS : null}
        zoomOnDoubleClick={false}
        snapToGrid
        snapGrid={grid.snap}
        panOnScroll
        panOnScrollSpeed={1}
        defaultViewport={saved ?? undefined}
        minZoom={MIN_ZOOM}
        maxZoom={MAX_ZOOM}
        proOptions={{ hideAttribution: true }}
        className="min-h-0 flex-1 bg-bg"
      >
        <Background variant={BackgroundVariant.Dots} gap={grid.step} size={1} className="!bg-bg" />
        <PeerLayer nodes={nodes} />
      </ReactFlow>
      {menu !== null && (
        <ContextMenu
          menu={menu}
          onClose={() => setMenu(null)}
          onReplace={setReplacing}
          onAdd={setAdding}
        />
      )}
      {adding !== null && <CanvasPalette at={adding} onClose={() => setAdding(null)} />}
      {replaced !== undefined && (
        <ReplaceDecoder node={replaced} onClose={() => setReplacing(null)} />
      )}
    </div>
  );
}

/** Tells peers which nodes this operator has selected, once per change. */
function usePublishSelection(publish: ReturnType<typeof usePointer>, selected: readonly string[]) {
  const key = selected.filter((id) => id !== "").join("\n");
  useEffect(() => {
    publish({ selected: key === "" ? [] : key.split("\n") });
  }, [publish, key]);
}

/** Escape clears the selection, unless a menu, palette or full screen face has the keyboard. */
function useEscapeDeselects(
  claimed: boolean,
  select: (id: string | null) => void,
  setNodes: React.Dispatch<React.SetStateAction<Node<FlowData>[]>>,
): void {
  useEffect(() => {
    if (claimed) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || isTyping(event.target)) {
        return;
      }
      setNodes((previous) =>
        previous.map((node) => (node.selected === true ? { ...node, selected: false } : node)),
      );
      select(null);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [claimed, select, setNodes]);
}

function addsToSelection(event: React.MouseEvent): boolean {
  return event.shiftKey || event.metaKey || event.ctrlKey;
}

interface Menu {
  x: number;
  y: number;
  target: { kind: "node"; id: string } | { kind: "edge"; id: string } | { kind: "pane" };
}

function usePointer(socket: { send: (command: ClientCommand) => void }) {
  const held = useRef<Pointer>({});
  const send = useMemo(
    () => createPointerSender((pointer) => socket.send({ type: "Point", data: pointer })),
    [socket],
  );
  const publish = useCallback(
    (change: Partial<Pointer>) => {
      held.current = { ...held.current, ...change };
      const { peers, you } = usePresenceStore.getState();
      if (othersOf(peers, you).length > 0) {
        send(held.current);
      }
    },
    [send],
  );
  useEffect(() => () => send({}), [send]);
  return publish;
}

function usePeerDrags(setNodes: ReturnType<typeof useNodesState<Node<FlowData>>>[1]) {
  const pointers = usePresenceStore((state) => state.pointers);
  const you = usePresenceStore((state) => state.you);
  const dragged = useMemo(() => draggedByOthers(pointers, you), [pointers, you]);
  useEffect(() => {
    if (dragged.size === 0) {
      return;
    }
    setNodes((previous) =>
      previous.map((node) => {
        const position = dragged.get(node.id);
        return position === undefined || node.dragging === true ? node : { ...node, position };
      }),
    );
  }, [dragged, setNodes]);
}

function useFitOnceMeasured(wanted: boolean) {
  const { fitView } = useReactFlow();
  const measured = useNodesInitialized();
  const fitted = useRef(!wanted);
  useEffect(() => {
    if (measured && !fitted.current) {
      fitted.current = true;
      void fitView(FIT_VIEW);
    }
  }, [measured, fitView]);
}

function ContextMenu({
  menu,
  onClose,
  onReplace,
  onAdd,
}: {
  menu: Menu;
  onClose: () => void;
  onReplace: (node: string) => void;
  onAdd: (at: ScreenPoint) => void;
}) {
  const workspace = useWorkspaceContext();
  const { fitView } = useReactFlow();
  const menuRef = useRef<HTMLDivElement>(null);
  const [place, setPlace] = useState<MenuPlace>({ left: menu.x, top: menu.y });
  const node = menu.target.kind === "node" ? nodeOf(workspace.graph, menu.target.id) : undefined;

  useLayoutEffect(() => {
    const box = menuRef.current?.getBoundingClientRect();
    if (box !== undefined) {
      setPlace(
        menuPlace({ left: menu.x, top: menu.y }, box, {
          width: window.innerWidth,
          height: window.innerHeight,
        }),
      );
    }
  }, [menu.x, menu.y]);
  const pinned = node !== undefined && isPinned(workspace.rack, node.id);

  useEffect(() => {
    const dismiss = (event: Event) => {
      if (event instanceof KeyboardEvent) {
        if (event.key === "Escape") {
          onClose();
        }
        return;
      }
      if (event.target instanceof Node && menuRef.current?.contains(event.target) === true) {
        return;
      }
      onClose();
    };
    window.addEventListener("keydown", dismiss);
    window.addEventListener("pointerdown", dismiss, { capture: true });
    return () => {
      window.removeEventListener("keydown", dismiss);
      window.removeEventListener("pointerdown", dismiss, { capture: true });
    };
  }, [onClose]);

  const item = (
    label: string,
    act: () => void,
    extra: { danger?: boolean; title?: string } = {},
  ) => (
    <Button
      key={label}
      type="button"
      title={extra.title}
      className={`${extra.danger === true ? BTN_QUIET_DANGER : BTN_QUIET} w-full justify-start`}
      onClick={() => {
        act();
        onClose();
      }}
    >
      {label}
    </Button>
  );

  const items: ReactNode[] = [];
  if (menu.target.kind === "pane") {
    items.push(item("Add node here", () => onAdd({ x: menu.x, y: menu.y })));
  }
  if (node !== undefined) {
    const full = workspace.expanded === node.id;
    if (node.kind === "channel") {
      items.push(
        item("Replace with…", () => onReplace(node.id), {
          title: "Swap this decoder for another: m and M cycle the analog modes",
        }),
      );
    }
    items.push(
      item(full ? "Leave full screen" : "Show full screen", () =>
        workspace.expand(full ? null : node.id),
      ),
    );
    items.push(
      item(pinned ? "Unpin from the rack" : "Pin to the rack", () =>
        workspace.edit((snapshot) => ({
          ...snapshot,
          rack: pinned ? unpin(snapshot.rack ?? {}, node.id) : pin(snapshot.rack ?? {}, node.id),
        })),
      ),
    );
    if (node.size != null && isResizable(node)) {
      items.push(
        item("Reset size", () =>
          workspace.edit((snapshot) => ({
            ...snapshot,
            graph: patchNode(snapshot.graph, node.id, ({ size: _size, ...rest }) => rest),
          })),
        ),
      );
    }
  }
  if (menu.target.kind === "edge") {
    const key = menu.target.id;
    items.push(
      item(
        "Delete wire",
        () => {
          workspace.edit((snapshot) => ({ ...snapshot, graph: removeEdge(snapshot.graph, key) }));
          workspace.apply();
        },
        { danger: true },
      ),
    );
  }
  items.push(item("Fit the patch on screen", () => void fitView(FIT_VIEW)));

  return (
    <div
      ref={menuRef}
      role="menu"
      className={`${SURFACE} fixed z-40 flex w-52 flex-col p-1`}
      style={place}
    >
      {items}
      {node !== undefined && (
        <span className="px-2 py-1 text-2xs text-ink-faint">
          Backspace deletes the selection: a node or a wire.
        </span>
      )}
    </div>
  );
}

function toFlowNodes(graph: PatchGraph, scale: number): Node<FlowData>[] {
  return graph.nodes.map((node) => {
    const size = (isResizable(node) ? node.size : null) ?? naturalSize(node, scale);
    return {
      id: node.id,
      type: flowTypeOf(node.kind),
      position: node.position,
      data: { node },
      width: isResizable(node) ? size.w : undefined,
      height: size.h,
      dragHandle: ".node-drag",
    };
  });
}
