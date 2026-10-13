import { catalogType } from "../lib/catalog";
import { randomHex } from "../lib/hex";
import { MAX_STREAMS } from "../lib/limits";
import type {
  Capabilities,
  ChannelDescriptor,
  DeviceSet,
  Face,
  NodeKind,
  NodeTypeInfo,
  PatchCatalog,
  PatchEdge,
  PatchGraph,
  PatchNode,
  PortDirection,
  PortRef,
  PortSpec,
  RackLayout,
  ResolvedPatch,
  ResolvedPort,
} from "../lib/types";

export interface GraphContext {
  catalog: PatchCatalog;
  channelTypes: readonly ChannelDescriptor[];
  resolved?: ResolvedPatch | null;
  bound?: ReadonlyMap<string, DeviceSet>;
}

export function nodeOf(graph: PatchGraph, id: string): PatchNode | undefined {
  return graph.nodes.find((node) => node.id === id);
}

export function portLabel(name: string, siblings: readonly PortSpec[]): string {
  const numbered = `${name}2`;
  return siblings.some((port) => port.name === numbered) ? `${name}1` : name;
}

export function echoesWire(graph: PatchGraph, node: string, port: PortSpec): boolean {
  return (
    port.direction === "in" &&
    (graph.edges ?? []).some(
      (edge) => edge.to.node === node && edge.to.port === port.name && edge.from.port === port.name,
    )
  );
}

export function streamLabel(base: string, index: number, streams: number): string {
  return `${base}${streams > 1 || index > 0 ? index + 1 : ""}`;
}

export function rxStreamCount(capabilities: Capabilities | undefined): number {
  return clampStreams(capabilities?.rx_streams);
}

function clampStreams(declared: number | undefined, floor = 1): number {
  return Math.min(Math.max(declared ?? floor, floor), MAX_STREAMS);
}

const ID_BYTES = 4;

export function nodeIds(graph: PatchGraph): Set<string> {
  return new Set(graph.nodes.map((node) => node.id));
}

export function newNodeId(kind: NodeKind, taken: ReadonlySet<string>): string {
  let id = `${kind}:${randomHex(ID_BYTES)}`;
  while (taken.has(id)) {
    id = `${kind}:${randomHex(ID_BYTES)}`;
  }
  return id;
}

export function descriptorOf(
  context: GraphContext,
  node: PatchNode,
): ChannelDescriptor | undefined {
  return node.kind === "channel"
    ? context.channelTypes.find((type) => type.type_id === node.data.channel_type)
    : undefined;
}

export function nodePorts(context: GraphContext, node: PatchNode): PortSpec[] {
  const resolved = context.resolved?.nodes.find((candidate) => candidate.id === node.id);
  return resolved === undefined || resolved.kind !== node.kind
    ? catalogPorts(context, node)
    : resolved.ports.map(specOf);
}

function specOf({
  feeds: _feeds,
  reaches: _reaches,
  ignored: _ignored,
  kept: _kept,
  ...spec
}: ResolvedPort): PortSpec {
  return spec;
}

export function catalogPorts(context: GraphContext, node: PatchNode): PortSpec[] {
  const entry = context.catalog.nodes.find((type) => type.kind === node.kind);
  const declared =
    entry?.needs_channel_type === true ? (descriptorOf(context, node)?.ports ?? []) : entry?.ports;
  return (declared ?? []).map((port) => ({ ...port, repeat: "once" as const }));
}

export function portOf(
  context: GraphContext,
  graph: PatchGraph,
  reference: PortRef,
  direction?: PortDirection,
): PortSpec | undefined {
  const node = nodeOf(graph, reference.node);
  return node === undefined
    ? undefined
    : nodePorts(context, node).find(
        (port) =>
          port.name === reference.port && (direction === undefined || port.direction === direction),
      );
}

export function handleSignature(ports: readonly PortSpec[]): string {
  return ports.map((port) => `${port.direction}:${port.name}`).join(",");
}

export function edgeKey(edge: PatchEdge): string {
  return `${edge.from.node}.${edge.from.port}->${edge.to.node}.${edge.to.port}`;
}

export interface NodeSize {
  w: number;
  h?: number;
}

export const FIT_MIN_W = 280;

const DIAL_MIN_W = 420;

export function nodeType(kind: NodeKind): NodeTypeInfo | undefined {
  return catalogType(kind);
}

export function opensDevice(kind: NodeKind): boolean {
  return nodeType(kind)?.role.role === "opens_radio";
}

export function faceOf(kind: NodeKind): Face {
  return nodeType(kind)?.face ?? { width: FIT_MIN_W };
}

export function nodeSize(kind: NodeKind): NodeSize {
  const face = faceOf(kind);
  return face.height == null ? { w: fixedWidth(face) } : { w: face.width, h: face.height };
}

function fixedWidth(face: Face): number {
  return Math.max(face.dial === true ? DIAL_MIN_W : FIT_MIN_W, face.width);
}

export function isResizable(kind: NodeKind): boolean {
  return faceOf(kind).height != null;
}

export const HEADER_PX = 26;
export const PORT_STEP_PX = 22;
export const PORT_TOP_PX = HEADER_PX + PORT_STEP_PX / 2;

export function nodeMinSize(kind: NodeKind, ports: readonly PortSpec[]): { w: number; h: number } {
  const face = faceOf(kind);
  const base =
    face.min == null ? { w: nodeSize(kind).w, h: 0 } : { w: face.min.width, h: face.min.height };
  const deepest = Math.max(
    ports.filter((port) => port.direction === "in").length,
    ports.filter((port) => port.direction === "out").length,
  );
  return { w: base.w, h: Math.max(base.h, PORT_TOP_PX + PORT_STEP_PX * deepest) };
}

export function addNode(graph: PatchGraph, node: PatchNode): PatchGraph {
  if (graph.nodes.some((drawn) => drawn.id === node.id)) {
    throw new Error(`duplicate node id ${node.id}`);
  }
  return { ...graph, nodes: [...graph.nodes, node] };
}

export function removeNode(graph: PatchGraph, id: string): PatchGraph {
  return {
    nodes: graph.nodes.filter((node) => node.id !== id),
    edges: (graph.edges ?? []).filter((edge) => edge.from.node !== id && edge.to.node !== id),
  };
}

export function addEdge(graph: PatchGraph, edge: PatchEdge): PatchGraph {
  return { ...graph, edges: [...(graph.edges ?? []), edge] };
}

export function removeEdge(graph: PatchGraph, key: string): PatchGraph {
  return {
    ...graph,
    edges: (graph.edges ?? []).filter((edge) => edgeKey(edge) !== key),
  };
}

export function patchNode(
  graph: PatchGraph,
  id: string,
  edit: (node: PatchNode) => PatchNode,
): PatchGraph {
  return {
    ...graph,
    nodes: graph.nodes.map((node) => (node.id === id ? edit(node) : node)),
  };
}

export function sameGraph(a: PatchGraph, b: PatchGraph): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

export const RACK_COLS = 12;
export const RACK_ROWS = 8;
export const RACK_DEFAULT = { w: 6, h: 4 } as const;

export interface RackCell {
  x: number;
  y: number;
  w: number;
  h: number;
}

export type RackEdge = "n" | "e" | "s" | "w";

export function isPinned(rack: RackLayout, node: string): boolean {
  return (rack.slots ?? []).some((slot) => slot.node === node);
}

export function pin(rack: RackLayout, node: string, size = RACK_DEFAULT): RackLayout {
  if (isPinned(rack, node)) {
    return rack;
  }
  const slots = rack.slots ?? [];
  for (let y = 0; y + size.h <= RACK_ROWS; y++) {
    for (let x = 0; x + size.w <= RACK_COLS; x++) {
      const cell = { x, y, w: size.w, h: size.h };
      if (!slots.some((slot) => overlaps(slot, cell))) {
        return { slots: [...slots, { node, ...cell }] };
      }
    }
  }
  return rack;
}

export function unpin(rack: RackLayout, node: string): RackLayout {
  return { slots: (rack.slots ?? []).filter((slot) => slot.node !== node) };
}

export function placeSlot(rack: RackLayout, node: string, cell: RackCell): RackLayout {
  const slots = rack.slots ?? [];
  if (!inside(cell) || !slots.every((slot) => slot.node === node || !overlaps(slot, cell))) {
    return rack;
  }
  return { slots: slots.map((slot) => (slot.node === node ? { node, ...cell } : slot)) };
}

export function moveSlot(rack: RackLayout, node: string, to: { x: number; y: number }): RackLayout {
  const slots = rack.slots ?? [];
  const from = slots.find((slot) => slot.node === node);
  if (from === undefined) {
    return rack;
  }
  const cell = { x: to.x, y: to.y, w: from.w, h: from.h };
  if (!inside(cell)) {
    return rack;
  }
  const hit = slots.filter((slot) => slot.node !== node && overlaps(slot, cell));
  if (hit.length === 0) {
    return { slots: slots.map((slot) => (slot.node === node ? { node, ...cell } : slot)) };
  }
  const other = hit.length === 1 ? hit[0] : undefined;
  if (other === undefined) {
    return rack;
  }
  return {
    slots: slots.map((slot) => {
      if (slot.node === node) {
        return { node, x: other.x, y: other.y, w: other.w, h: other.h };
      }
      return slot.node === other.node
        ? { node: other.node, x: from.x, y: from.y, w: from.w, h: from.h }
        : slot;
    }),
  };
}

export function resizeSlot(
  rack: RackLayout,
  node: string,
  edge: RackEdge,
  cells: number,
): RackLayout {
  const slots = rack.slots ?? [];
  const slot = slots.find((candidate) => candidate.node === node);
  if (slot === undefined || cells === 0) {
    return rack;
  }
  const pushed = new Set(
    slots.filter((other) => other.node !== node && abuts(slot, other, edge)).map((o) => o.node),
  );
  const next = slots.map((candidate) => {
    if (candidate.node === node) {
      return { node, ...moveEdge(candidate, edge, cells) };
    }
    return pushed.has(candidate.node)
      ? { node: candidate.node, ...moveEdge(candidate, OPPOSITE[edge], cells) }
      : candidate;
  });
  const legal = next.every(
    (cell, index) =>
      inside(cell) && next.every((other, at) => at === index || !overlaps(cell, other)),
  );
  return legal ? { slots: next } : rack;
}

export function slotRoom(
  rack: RackLayout,
  node: string,
  edge: RackEdge,
): { min: number; max: number } {
  const slots = rack.slots ?? [];
  const slot = slots.find((candidate) => candidate.node === node);
  if (slot === undefined) {
    return { min: 0, max: 0 };
  }
  const rooms = [
    edgeRoom(slot, edge),
    ...slots
      .filter((other) => other.node !== node && abuts(slot, other, edge))
      .map((other) => edgeRoom(other, OPPOSITE[edge])),
  ];
  return {
    min: noNegativeZero(Math.max(...rooms.map((room) => room.min))),
    max: noNegativeZero(Math.min(...rooms.map((room) => room.max))),
  };
}

function noNegativeZero(value: number): number {
  return value === 0 ? 0 : value;
}

export function clampCells(room: { min: number; max: number }, cells: number): number {
  return Math.min(Math.max(cells, room.min), Math.max(room.max, room.min));
}

export function pruneRack(rack: RackLayout, graph: PatchGraph): RackLayout {
  const known = new Set(graph.nodes.map((node) => node.id));
  const kept = (rack.slots ?? []).filter((slot) => known.has(slot.node));
  if (kept.length === (rack.slots ?? []).length && kept.every((slot) => inside(slot))) {
    return rack;
  }
  let placed: RackLayout = { slots: kept.filter((slot) => inside(slot)) };
  for (const slot of kept) {
    if (!inside(slot)) {
      placed = pin(placed, slot.node);
    }
  }
  return placed;
}

function inside(cell: RackCell): boolean {
  return (
    cell.w >= 1 &&
    cell.h >= 1 &&
    cell.x >= 0 &&
    cell.y >= 0 &&
    cell.x + cell.w <= RACK_COLS &&
    cell.y + cell.h <= RACK_ROWS
  );
}

const OPPOSITE: Record<RackEdge, RackEdge> = { n: "s", e: "w", s: "n", w: "e" };

function edgeRoom(cell: RackCell, edge: RackEdge): { min: number; max: number } {
  switch (edge) {
    case "n":
      return { min: -cell.y, max: cell.h - 1 };
    case "s":
      return { min: 1 - cell.h, max: RACK_ROWS - cell.y - cell.h };
    case "w":
      return { min: -cell.x, max: cell.w - 1 };
    case "e":
      return { min: 1 - cell.w, max: RACK_COLS - cell.x - cell.w };
  }
}

function moveEdge(cell: RackCell, edge: RackEdge, cells: number): RackCell {
  switch (edge) {
    case "n":
      return { ...cell, y: cell.y + cells, h: cell.h - cells };
    case "s":
      return { ...cell, h: cell.h + cells };
    case "w":
      return { ...cell, x: cell.x + cells, w: cell.w - cells };
    case "e":
      return { ...cell, w: cell.w + cells };
  }
}

function abuts(cell: RackCell, other: RackCell, edge: RackEdge): boolean {
  const spansX = other.x < cell.x + cell.w && cell.x < other.x + other.w;
  const spansY = other.y < cell.y + cell.h && cell.y < other.y + other.h;
  switch (edge) {
    case "n":
      return spansX && other.y + other.h === cell.y;
    case "s":
      return spansX && other.y === cell.y + cell.h;
    case "w":
      return spansY && other.x + other.w === cell.x;
    case "e":
      return spansY && other.x === cell.x + cell.w;
  }
}

function overlaps(a: RackCell, b: RackCell): boolean {
  return a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
}
