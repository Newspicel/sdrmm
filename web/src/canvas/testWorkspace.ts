import { MAX_EDGES, MAX_NODES, WORKSPACE_SNAPSHOT_VERSION } from "../lib/limits";
import type {
  NodeBody,
  NodeTypeInfo,
  PatchEdge,
  PatchGraph,
  PatchNode,
  PortRef,
  PortSpec,
  WireVerdict,
  WorkspaceSnapshot,
} from "../lib/types";
import { declaredPorts, faceOf, type GraphContext, isResizable, naturalSize } from "./graph";
import { newNodeBody } from "./newNode";
import { processorKindOf } from "./nodes/processorFace";

export type TestContext = Pick<GraphContext, "catalog" | "channelTypes" | "encoderTypes">;

export const SECTIONS = ["sources", "unwired", "stopped", "running"] as const;
export type Section = (typeof SECTIONS)[number];
type State = "stopped" | "running";
const STATES: readonly State[] = ["stopped", "running"];

const LABELS: Record<Section, string> = {
  sources: "## Sources",
  unwired: "## Unwired",
  stopped: "## Stopped",
  running: "## Running",
};

export interface TestWorkspace {
  name: string;
  snapshot: WorkspaceSnapshot;
}

/** The radio every wired copy reads, the bench array an input that takes one wire per lane
 * reads instead, and the two dongles a spread-tuned array reads, since the bench array tunes
 * its lanes together. All are virtual, so the patch needs no hardware. */
export const TEST_RADIOS = {
  generator: { key: "gen-test", streams: 1 },
  lanes: { key: "kraken5", streams: 5 },
  spread: { keys: ["dongle1", "dongle2"] },
} as const;

const SPREAD = "-spread";

const VIRTUAL = "virtual";
const NAME = "Test workspace";
const FEEDER_DEPTH = 3;
const LARGE = 1.6;
const GAP = 48;
const SECTION_GAP = 160;
const COLUMNS = ["unwired", "stopped", "running"] as const;
type Column = (typeof COLUMNS)[number];
/// A band of rows stops near this height and the next starts beside it.
const BAND_H = 12_000;
const MAX_BANDS = 8;
/// Note nodes the layout adds: the sources head and three column heads per band.
const LABELS_HELD = 1 + COLUMNS.length * MAX_BANDS;
const ROW_W = 8400;
const LABEL = { w: 240, h: 120 };

/// A face that is not resizable grows with its content, which the patch cannot measure, so the
/// layout leaves the room the tallest of its kind takes.
const ROOM_H = { dial: 240, plain: 360 };

/** The server's verdict on every input `from` could feed in `graph`. */
export type Wiring = (graph: PatchGraph, from: PortRef) => Promise<readonly WireVerdict[]>;

/// Thrown by a draft that grew past the patch limits, so the subject starts the next workspace.
const FULL = new Error("the test patch is full");

interface Template {
  id: string;
  body: NodeBody;
  radio: boolean;
  picked: boolean;
  ports: PortSpec[];
}

/** Patches that together hold every node kind, decoder, encoder and processor the catalog lists:
 * unwired, then wired to a stopped and to a running virtual radio. As many as the patch limits
 * need, each a new workspace. Every wire is one `wiring` admits. */
export async function testWorkspaces(
  context: TestContext,
  wiring: Wiring,
  scale = 1,
): Promise<TestWorkspace[]> {
  const providers = templatesOf(context);
  const drafts: Draft[] = [];
  let draft = new Draft(context, providers, wiring);
  for (const subject of subjectsOf(providers)) {
    const trial = draft.clone();
    if (await fitted(trial, subject)) {
      draft = trial;
      continue;
    }
    if (draft.empty()) {
      throw FULL;
    }
    drafts.push(draft);
    draft = new Draft(context, providers, wiring);
    if (!(await fitted(draft, subject))) {
      throw FULL;
    }
  }
  if (!draft.empty()) {
    drafts.push(draft);
  }
  return drafts.map((done, index) => ({
    name: drafts.length === 1 ? NAME : `${NAME} ${index + 1}/${drafts.length}`,
    snapshot: done.snapshot(scale),
  }));
}

async function fitted(draft: Draft, subject: Template): Promise<boolean> {
  try {
    await draft.add(subject);
    return true;
  } catch (error) {
    if (error === FULL) {
      return false;
    }
    throw error;
  }
}

export function sectionOf(node: PatchNode): Section | undefined {
  const prefix = node.id.split("-")[0];
  return SECTIONS.find((section) => section === prefix);
}

/** The room a node of the test patch takes on the canvas. */
export function footprint(node: PatchNode, scale: number): { w: number; h: number } {
  if (node.size != null) {
    return { w: node.size.w, h: node.size.h };
  }
  const natural = naturalSize(node, scale);
  const room = faceOf(node).dial === true ? ROOM_H.dial : ROOM_H.plain;
  return { w: natural.w, h: natural.h ?? Math.round(room * scale) };
}

export function streamPort(base: string, index: number): string {
  return index === 0 ? base : `${base}${index + 1}`;
}

function refKey(ref: PortRef): string {
  return `${ref.node}\n${ref.port}`;
}

/** An output of the type `input` takes; whether it may feed it is the server's call. */
function typed(out: PortSpec, input: PortSpec): boolean {
  return out.direction === "out" && out.port_type === input.port_type;
}

/** Two inputs where a wire into one leaves the other empty: the same port, or one group. */
function rivals(port: PortSpec, input: PortSpec): boolean {
  return port === input || (input.group != null && port.group === input.group);
}

function inputs(ports: readonly PortSpec[]): PortSpec[] {
  return ports.filter((port) => port.direction === "in");
}

function templatesOf(context: TestContext): Template[] {
  return context.catalog.nodes.flatMap((entry) => kindTemplates(context, entry));
}

function subjectsOf(templates: readonly Template[]): Template[] {
  return [
    ...templates.filter((template) => !template.picked),
    ...templates.filter((template) => template.picked),
  ];
}

function kindTemplates(context: TestContext, entry: NodeTypeInfo): Template[] {
  const picked = pickedTypes(context, entry);
  return (picked ?? [undefined]).flatMap((typeId) => {
    const body = newNodeBody(context.catalog, entry.kind, typeId === undefined ? {} : { typeId });
    if (body === null) {
      return [];
    }
    const id = typeId === undefined ? entry.kind : `${entry.kind}-${typeId}`;
    return [
      {
        id,
        body,
        radio: entry.role.role === "opens_radio",
        picked: typeId !== undefined,
        ports: declaredPorts(context, placed(id, body)),
      },
    ];
  });
}

function pickedTypes(context: TestContext, entry: NodeTypeInfo): string[] | undefined {
  switch (entry.picks) {
    case "decoder":
      return context.channelTypes.map((type) => type.type_id);
    case "encoder":
      return (context.encoderTypes ?? []).map((type) => type.type_id);
    case "processor":
      return (context.catalog.processors ?? []).map((type) => type.type_id);
    default:
      return undefined;
  }
}

function placed(id: string, body: NodeBody): PatchNode {
  return { id, position: { x: 0, y: 0 }, ...body };
}

function radioBody(body: NodeBody, key: string, stopped: boolean): NodeBody {
  return body.kind === "device"
    ? { kind: "device", data: { ...body.data, device: { backend: VIRTUAL, key }, stopped } }
    : body;
}

/** Verdicts by output, then by input, with the wire count they were asked at. One lineage of
 * drafts shares it: wires are only added, so a refusal holds, but an admission is asked again
 * once wires were added, since an output may fill up (a radio holds so many channels). */
type Verdicts = Map<string, { wires: number; to: Map<string, string | null> }>;

class Draft {
  private nodes: PatchNode[] = [];
  private wires: PatchEdge[] = [];
  private ports = new Map<string, PortSpec[]>();
  private verdicts: Verdicts = new Map();
  private readonly radio: Template | undefined;

  constructor(
    private readonly context: TestContext,
    private readonly providers: readonly Template[],
    private readonly wiring: Wiring,
  ) {
    this.radio = providers.find((template) => template.radio);
  }

  clone(): Draft {
    const copy = new Draft(this.context, this.providers, this.wiring);
    copy.nodes = [...this.nodes];
    copy.wires = [...this.wires];
    copy.ports = new Map(this.ports);
    copy.verdicts = this.verdicts;
    return copy;
  }

  empty(): boolean {
    return this.nodes.length === 0;
  }

  async add(subject: Template): Promise<void> {
    if (subject.radio) {
      this.place("unwired", subject);
      return;
    }
    if (inputs(subject.ports).length === 0) {
      this.place("sources", subject);
      return;
    }
    this.place("unwired", subject);
    for (const state of STATES) {
      await this.feed(this.place(state, subject), state, 0);
    }
  }

  snapshot(scale: number): WorkspaceSnapshot {
    return {
      version: WORKSPACE_SNAPSHOT_VERSION,
      graph: { nodes: laidOut(this.sized(scale), scale), edges: this.wires },
    };
  }

  private sized(scale: number): PatchNode[] {
    return this.nodes.map((node) => {
      if (!isResizable(node)) {
        return node;
      }
      const natural = naturalSize(node, scale);
      const grow = (this.ports.get(node.id) ?? []).length > 0 ? LARGE : 1;
      return {
        ...node,
        size: { w: Math.round(natural.w * grow), h: Math.round((natural.h ?? 0) * grow) },
      };
    });
  }

  private node(id: string): PatchNode | undefined {
    return this.nodes.find((node) => node.id === id);
  }

  private place(section: Section, template: Template): PatchNode {
    if (section === "stopped" || section === "running") {
      this.rootOf(section, TEST_RADIOS.generator.key, "");
    }
    return this.put(`${section}-${template.id}`, template.body, template.ports);
  }

  private put(id: string, body: NodeBody, ports: PortSpec[]): PatchNode {
    const held = this.node(id);
    if (held !== undefined) {
      return held;
    }
    const node = placed(id, body);
    this.nodes.push(node);
    this.ports.set(id, ports);
    if (this.nodes.length + LABELS_HELD > MAX_NODES) {
      throw FULL;
    }
    return node;
  }

  private wire(from: PortRef, to: PortRef): void {
    this.wires.push({ from, to });
    if (this.wires.length > MAX_EDGES) {
      throw FULL;
    }
  }

  private rootOf(state: State, key: string, suffix: string): PatchNode | undefined {
    const radio = this.radio;
    if (radio === undefined) {
      return undefined;
    }
    const id = `${state}-${radio.id}${suffix}`;
    return (
      this.node(id) ?? this.put(id, radioBody(radio.body, key, state === "stopped"), radio.ports)
    );
  }

  private async feed(node: PatchNode, state: State, depth: number): Promise<void> {
    const ports = inputs(this.ports.get(node.id) ?? []);
    for (const input of ports) {
      if (ports.some((port) => rivals(port, input) && this.holds(node.id, port.name))) {
        continue;
      }
      if (input.repeat === "per_lane") {
        this.feedLanes(node, input, state);
        continue;
      }
      const from = await this.provider(node, input, state, depth);
      if (from !== undefined) {
        this.wire(from, { node: node.id, port: input.name });
      }
    }
  }

  private holds(node: string, port: string): boolean {
    return this.wires.some((edge) => edge.to.node === node && edge.to.port === port);
  }

  private feedLanes(node: PatchNode, input: PortSpec, state: State): void {
    if (node.id.endsWith(SPREAD)) {
      this.feedSpreadLanes(node, input, state);
      return;
    }
    const radio = this.rootOf(state, TEST_RADIOS.lanes.key, "-lanes");
    const out = (this.ports.get(radio?.id ?? "") ?? []).find(
      (port) => port.repeat === "per_rx_stream" && typed(port, input),
    );
    if (radio === undefined || out === undefined) {
      return;
    }
    for (let lane = 0; lane < TEST_RADIOS.lanes.streams; lane++) {
      this.wire(
        { node: radio.id, port: streamPort(out.name, lane) },
        { node: node.id, port: streamPort(input.name, lane) },
      );
    }
  }

  /** An output the server lets feed `input`: a node already drawn first, else a new feeder,
   * in catalog order. A drawn template whose outputs are taken is passed over, not drawn twice. */
  private async provider(
    target: PatchNode,
    input: PortSpec,
    state: State,
    depth: number,
  ): Promise<PortRef | undefined> {
    const able = this.providers.filter((template) =>
      template.ports.some((port) => typed(port, input)),
    );
    if (this.wantsSpread(target)) {
      const spread = able.find((template) => template.body.kind === "array");
      if (spread !== undefined) {
        return this.spreadArray(spread, target, input, state, depth);
      }
    }
    return (
      (await this.heldOutput(able, target, input, state)) ??
      (await this.spareRadio(able, target, input, state)) ??
      (await this.feederOutput(able, target, input, state, depth))
    );
  }

  private wantsSpread(target: PatchNode): boolean {
    return processorKindOf(this.context.catalog, target).facets.tuning === "spread";
  }

  /** An array of its own, tuned spread, for a processor that needs its lanes spread. */
  private async spreadArray(
    template: Template,
    target: PatchNode,
    input: PortSpec,
    state: State,
    depth: number,
  ): Promise<PortRef | undefined> {
    if (template.body.kind !== "array") {
      return undefined;
    }
    const id = `${homeOf(template, state)}-${template.id}${SPREAD}`;
    const held = this.node(id);
    if (held !== undefined) {
      const port = await this.freeOutput(held, input, target);
      return port === undefined ? undefined : { node: held.id, port };
    }
    const body: NodeBody = { ...template.body, data: { ...template.body.data, tuning: "spread" } };
    const port = await this.admittedPort(placed(id, body), template.ports, input, target);
    if (port === undefined) {
      return undefined;
    }
    const array = this.put(id, body, template.ports);
    await this.feed(array, state, depth + 1);
    return { node: array.id, port };
  }

  private feedSpreadLanes(node: PatchNode, input: PortSpec, state: State): void {
    TEST_RADIOS.spread.keys.forEach((key, lane) => {
      const radio = this.rootOf(state, key, `-${key}`);
      const out = (this.ports.get(radio?.id ?? "") ?? []).find(
        (port) => port.repeat === "per_rx_stream" && typed(port, input),
      );
      if (radio !== undefined && out !== undefined) {
        this.wire(
          { node: radio.id, port: streamPort(out.name, 0) },
          { node: node.id, port: streamPort(input.name, lane) },
        );
      }
    });
  }

  /** Every drawn copy of `template` in the home it takes in `state`: the radio may be drawn more
   * than once, `running-device`, `running-device-2`, ..., each holding so many channels. */
  private drawn(template: Template, state: State): PatchNode[] {
    const base = `${homeOf(template, state)}-${template.id}`;
    const copies: PatchNode[] = [];
    for (let held = this.node(base); held !== undefined; ) {
      copies.push(held);
      held = template === this.radio ? this.node(`${base}-${copies.length + 1}`) : undefined;
    }
    return copies;
  }

  /** Another radio like the drawn ones, when every one of them refuses `input`. */
  private async spareRadio(
    able: readonly Template[],
    target: PatchNode,
    input: PortSpec,
    state: State,
  ): Promise<PortRef | undefined> {
    const radio = this.radio;
    const drawn = radio === undefined ? [] : this.drawn(radio, state);
    if (radio === undefined || drawn.length === 0 || !able.includes(radio)) {
      return undefined;
    }
    const suffix = `-${drawn.length + 1}`;
    const body = radioBody(radio.body, TEST_RADIOS.generator.key, state === "stopped");
    const probe = placed(`${state}-${radio.id}${suffix}`, body);
    const port = await this.admittedPort(probe, radio.ports, input, target);
    if (port === undefined) {
      return undefined;
    }
    const spare = this.rootOf(state, TEST_RADIOS.generator.key, suffix);
    return spare && { node: spare.id, port };
  }

  private async heldOutput(
    able: readonly Template[],
    target: PatchNode,
    input: PortSpec,
    state: State,
  ): Promise<PortRef | undefined> {
    for (const template of able) {
      for (const held of this.drawn(template, state)) {
        const port = await this.freeOutput(held, input, target);
        if (port !== undefined) {
          return { node: held.id, port };
        }
      }
    }
    return undefined;
  }

  /** A new node of the first template the server lets feed `input`, asked before it is drawn so
   * a refused one leaves no trace. */
  private async feederOutput(
    able: readonly Template[],
    target: PatchNode,
    input: PortSpec,
    state: State,
    depth: number,
  ): Promise<PortRef | undefined> {
    for (const template of able) {
      const home = homeOf(template, state);
      const id = `${home}-${template.id}`;
      if (template.radio || depth >= FEEDER_DEPTH || this.node(id)) {
        continue;
      }
      const probe = placed(id, template.body);
      const port = await this.admittedPort(probe, template.ports, input, target);
      if (port === undefined) {
        continue;
      }
      const feeder = this.place(home, template);
      if (home !== "sources") {
        await this.feed(feeder, state, depth + 1);
      }
      return { node: feeder.id, port };
    }
    return undefined;
  }

  private async freeOutput(
    source: PatchNode,
    input: PortSpec,
    target: PatchNode,
  ): Promise<string | undefined> {
    if (source.id === target.id || this.reaches(target.id, source.id)) {
      return undefined;
    }
    const free = (this.ports.get(source.id) ?? []).filter(
      (port) =>
        port.multi !== "one" ||
        !this.wires.some((edge) => edge.from.node === source.id && edge.from.port === port.name),
    );
    return this.admittedPort(source, free, input, target);
  }

  private async admittedPort(
    source: PatchNode,
    ports: readonly PortSpec[],
    input: PortSpec,
    target: PatchNode,
  ): Promise<string | undefined> {
    const to = { node: target.id, port: input.name };
    for (const port of ports.filter((candidate) => typed(candidate, input))) {
      if ((await this.refusal({ node: source.id, port: port.name }, to, source)) === null) {
        return port.name;
      }
    }
    return undefined;
  }

  /** Why the server refuses `from` into `to`, or null; `source` joins the graph asked about
   * when it is not drawn yet. An input the server does not list is refused. */
  private async refusal(from: PortRef, to: PortRef, source: PatchNode): Promise<string | null> {
    const known = this.verdicts.get(refKey(from));
    const held = known?.to.get(refKey(to));
    if (typeof held === "string" || (held === null && known?.wires === this.wires.length)) {
      return held;
    }
    const nodes = this.node(source.id) === undefined ? [...this.nodes, source] : this.nodes;
    const verdicts = await this.wiring({ nodes, edges: this.wires }, from);
    const fresh = new Map(verdicts.map((verdict) => [refKey(verdict.to), verdict.refusal ?? null]));
    this.verdicts.set(refKey(from), { wires: this.wires.length, to: fresh });
    const verdict = fresh.get(refKey(to));
    return verdict === undefined ? "not offered" : verdict;
  }

  private reaches(from: string, to: string): boolean {
    const seen = new Set<string>();
    const stack = [from];
    for (let at = stack.pop(); at !== undefined; at = stack.pop()) {
      if (at === to) {
        return true;
      }
      if (!seen.has(at)) {
        seen.add(at);
        stack.push(
          ...this.wires.filter((edge) => edge.from.node === at).map((edge) => edge.to.node),
        );
      }
    }
    return false;
  }
}

function homeOf(template: Template, state: State): Section {
  return !template.radio && inputs(template.ports).length === 0 ? "sources" : state;
}

/** Sources first, then one row per node: its unwired, stopped and running copies side by side
 * under their column heads. Rows fold into bands side by side so the canvas stays in bounds. */
function laidOut(nodes: readonly PatchNode[], scale: number): PatchNode[] {
  const out: PatchNode[] = [];
  let top = 0;
  const sources = nodes
    .filter((node) => sectionOf(node) === "sources")
    .toSorted((a, b) => footprint(b, scale).h - footprint(a, scale).h);
  if (sources.length > 0) {
    out.push(label("sources", "sources-label", 0, top, scale));
    const shelf = shelved(sources, top + Math.round((LABEL.h + GAP) * scale), scale);
    out.push(...shelf.nodes);
    top = shelf.bottom + Math.round(SECTION_GAP * scale);
  }
  return [...out, ...grid(rowsOf(nodes), top, scale)];
}

type Row = Partial<Record<Column, PatchNode>>;

/** Each node's copies by the name they share past their section, in the order they were drawn. */
function rowsOf(nodes: readonly PatchNode[]): Row[] {
  const rows = new Map<string, Row>();
  for (const node of nodes) {
    const column = COLUMNS.find((held) => held === sectionOf(node));
    if (column === undefined) {
      continue;
    }
    const key = node.id.slice(column.length + 1);
    rows.set(key, { ...rows.get(key), [column]: node });
  }
  return [...rows.values()];
}

function grid(rows: readonly Row[], top: number, scale: number): PatchNode[] {
  const gap = Math.round(GAP * scale);
  const widths = COLUMNS.map((column) =>
    Math.max(Math.round(LABEL.w * scale), ...rows.map((row) => boxOf(row[column], scale).w)),
  );
  const lefts = widths.map((_, index) => widths.slice(0, index).reduce((x, w) => x + w + gap, 0));
  const bandW = (lefts.at(-1) ?? 0) + (widths.at(-1) ?? 0) + Math.round(SECTION_GAP * scale);
  const heights = rows.map((row) =>
    Math.max(...COLUMNS.map((column) => boxOf(row[column], scale).h)),
  );
  const total = heights.reduce((sum, h) => sum + h + gap, 0);
  const bands = Math.min(MAX_BANDS, Math.max(1, Math.ceil(total / (BAND_H * scale))));
  const head = Math.round((LABEL.h + GAP) * scale);
  const out: PatchNode[] = [];
  let band = -1;
  let y = 0;
  rows.forEach((row, index) => {
    const full = band >= 0 && y - top - head > total / bands && band < bands - 1;
    if (band < 0 || full) {
      band += 1;
      out.push(
        ...COLUMNS.map((column, at) =>
          label(column, `${column}-label-${band}`, band * bandW + (lefts[at] ?? 0), top, scale),
        ),
      );
      y = top + head;
    }
    COLUMNS.forEach((column, at) => {
      const node = row[column];
      if (node !== undefined) {
        out.push({ ...node, position: { x: band * bandW + (lefts[at] ?? 0), y } });
      }
    });
    y += (heights[index] ?? 0) + gap;
  });
  return out;
}

function boxOf(node: PatchNode | undefined, scale: number): { w: number; h: number } {
  return node === undefined ? { w: 0, h: 0 } : footprint(node, scale);
}

function label(section: Section, id: string, x: number, y: number, scale: number): PatchNode {
  return {
    id,
    kind: "note",
    data: { text: LABELS[section] },
    position: { x, y },
    size: { w: Math.round(LABEL.w * scale), h: Math.round(LABEL.h * scale) },
  };
}

function shelved(
  nodes: readonly PatchNode[],
  top: number,
  scale: number,
): { nodes: PatchNode[]; bottom: number } {
  const gap = Math.round(GAP * scale);
  const out: PatchNode[] = [];
  let x = 0;
  let y = top;
  let row = 0;
  for (const node of nodes) {
    const box = footprint(node, scale);
    if (x > 0 && x + box.w > ROW_W * scale) {
      x = 0;
      y += row + gap;
      row = 0;
    }
    out.push({ ...node, position: { x, y } });
    x += box.w + gap;
    row = Math.max(row, box.h);
  }
  return { nodes: out, bottom: y + row };
}
