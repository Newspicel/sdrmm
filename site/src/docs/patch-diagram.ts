export type NodeCategory = "source" | "channel" | "encoder" | "tool" | "output";

export type PortKind =
  | "iq"
  | "baseband"
  | "audio"
  | "events"
  | "video"
  | "control"
  | "position"
  | "array"
  | "bearing"
  | "transmit";

export interface PatchNodeSpec {
  id: string;
  label: string;
  sub?: string;
  category: NodeCategory;
}

export interface PatchEdgeSpec {
  from: string;
  to: string;
  port: PortKind;
  label?: string;
}

export interface PatchGraphSpec {
  nodes: PatchNodeSpec[];
  edges: PatchEdgeSpec[];
}

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

interface PlacedNode extends PatchNodeSpec {
  col: number;
  row: number;
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface PatchLayout {
  width: number;
  height: number;
  nodes: PlacedNode[];
  wires: { edge: PatchEdgeSpec; path: string; x1: number; y1: number; x2: number; y2: number }[];
  tags: { edge: PatchEdgeSpec; text: string; cx: number; cy: number; rect: Rect }[];
}

const CATEGORIES = new Set<NodeCategory>(["source", "channel", "encoder", "tool", "output"]);

const PORTS = new Set<PortKind>([
  "iq",
  "baseband",
  "audio",
  "events",
  "video",
  "control",
  "position",
  "array",
  "bearing",
  "transmit",
]);

const PORT_GLYPHS: Record<PortKind, string> = {
  iq: "M4 0A4 4 0 1 1 -4 0A4 4 0 1 1 4 0Z",
  baseband: "M4 0A4 4 0 1 1 -4 0A4 4 0 1 1 4 0Z",
  audio: "M0 -4.5 4.5 0 0 4.5 -4.5 0Z",
  events: "M-2.8 -4.2h5.6l1.8 4.2-1.8 4.2h-5.6l-1.8-4.2Z",
  video: "M-4 -3.5h8v7h-8Z",
  control: "M-4 -4 4 0 -4 4Z",
  position: "M0 -4.5 4 3.5 -4 3.5Z",
  array: "M-4 -3.5h8v7h-8Z",
  bearing: "M0 -4.5 4.5 0 0 4.5 -4.5 0Z",
  transmit: "M-4 -4 4 0 -4 4Z",
};

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function asCategory(raw: string): NodeCategory {
  const lower = raw.trim().toLowerCase() as NodeCategory;
  return CATEGORIES.has(lower) ? lower : "tool";
}

function asPort(raw: string): PortKind {
  const lower = raw.trim().toLowerCase() as PortKind;
  return PORTS.has(lower) ? lower : "iq";
}

export function parsePatch(source: string): PatchGraphSpec {
  const nodes = new Map<string, PatchNodeSpec>();
  const edges: PatchEdgeSpec[] = [];

  for (const rawLine of source.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (line === "" || line.startsWith("#")) {
      continue;
    }

    const edgeMatch = line.match(/^(.+?)\s*--([a-zA-Z0-9_]+)(?::([^->]+))?-->\s*(.+)$/);
    if (edgeMatch) {
      const from = (edgeMatch[1] ?? "").trim();
      const port = asPort(edgeMatch[2] ?? "iq");
      const label = edgeMatch[3]?.trim();
      const to = (edgeMatch[4] ?? "").trim();
      if (!nodes.has(from)) {
        nodes.set(from, { id: from, label: from, category: "tool" });
      }
      if (!nodes.has(to)) {
        nodes.set(to, { id: to, label: to, category: "tool" });
      }
      edges.push({ from, to, port, label });
      continue;
    }

    const nodeMatch = line.match(/^\[([a-zA-Z]+)\]\s*([^:]+?)(?:\s*:\s*(.+))?$/);
    if (nodeMatch) {
      const category = asCategory(nodeMatch[1] ?? "tool");
      const name = (nodeMatch[2] ?? "").trim();
      const sub = nodeMatch[3]?.trim();
      nodes.set(name, { id: name, label: name, sub, category });
    }
  }

  return { nodes: [...nodes.values()], edges };
}

function assignColumns(spec: PatchGraphSpec): Map<string, number> {
  const depth = new Map<string, number>();
  for (const node of spec.nodes) {
    depth.set(node.id, 0);
  }
  for (let pass = 0; pass < spec.nodes.length; pass++) {
    let changed = false;
    for (const edge of spec.edges) {
      const fromCol = depth.get(edge.from) ?? 0;
      const toCol = depth.get(edge.to) ?? 0;
      if (toCol <= fromCol) {
        depth.set(edge.to, fromCol + 1);
        changed = true;
      }
    }
    if (!changed) break;
  }
  return depth;
}

function orderColumns(
  byCol: Map<number, PatchNodeSpec[]>,
  colKeys: number[],
  edges: PatchEdgeSpec[],
) {
  const rank = new Map<string, number>();
  const refreshRanks = () => {
    for (const col of colKeys) {
      const list = byCol.get(col) ?? [];
      list.forEach((node, idx) => rank.set(node.id, idx));
    }
  };
  refreshRanks();

  for (let sweep = 0; sweep < 2; sweep++) {
    for (let i = 1; i < colKeys.length; i++) {
      const col = colKeys[i];
      if (col === undefined) continue;
      const list = byCol.get(col) ?? [];
      list.sort((a, b) => {
        const avg = (id: string) => {
          const parents = edges.filter((e) => e.to === id).map((e) => rank.get(e.from) ?? 0);
          return parents.length === 0
            ? (rank.get(id) ?? 0)
            : parents.reduce((s, v) => s + v, 0) / parents.length;
        };
        return avg(a.id) - avg(b.id);
      });
      refreshRanks();
    }
  }
}

function rectsOverlap(a: Rect, b: Rect, pad = 2): boolean {
  return (
    a.x - pad < b.x + b.w && a.x + a.w + pad > b.x && a.y - pad < b.y + b.h && a.y + a.h + pad > b.y
  );
}

function cubicPoint(
  x1: number,
  y1: number,
  cx1: number,
  cy1: number,
  cx2: number,
  cy2: number,
  x2: number,
  y2: number,
  t: number,
): { x: number; y: number } {
  const u = 1 - t;
  const tt = t * t;
  const uu = u * u;
  const uuu = uu * u;
  const ttt = tt * t;
  return {
    x: uuu * x1 + 3 * uu * t * cx1 + 3 * u * tt * cx2 + ttt * x2,
    y: uuu * y1 + 3 * uu * t * cy1 + 3 * u * tt * cy2 + ttt * y2,
  };
}

function pickCorridorY(
  midNodes: PlacedNode[],
  targetY: number,
  usedTop: Map<string, number>,
  usedBot: Map<string, number>,
  spanKey: string,
): number {
  const LANE_GAP = 22;
  const minY = Math.min(...midNodes.map((n) => n.y));
  const maxBot = Math.max(...midNodes.map((n) => n.y + n.h));

  const candidates: { y: number; kind: "top" | "gap" | "bot" }[] = [];
  const topSlot = usedTop.get(spanKey) ?? 0;
  candidates.push({ y: minY - 26 - topSlot * LANE_GAP, kind: "top" });

  const sorted = [...midNodes].sort((a, b) => a.y - b.y);
  let sweepEnd = sorted[0] ? sorted[0].y + sorted[0].h : 0;
  for (let i = 1; i < sorted.length; i++) {
    const node = sorted[i];
    if (!node) continue;
    if (node.y - sweepEnd >= 28) {
      candidates.push({ y: (sweepEnd + node.y) / 2, kind: "gap" });
    }
    sweepEnd = Math.max(sweepEnd, node.y + node.h);
  }

  const botSlot = usedBot.get(spanKey) ?? 0;
  candidates.push({ y: maxBot + 26 + botSlot * LANE_GAP, kind: "bot" });

  candidates.sort((a, b) => Math.abs(a.y - targetY) - Math.abs(b.y - targetY));
  const best = candidates[0] ?? { y: minY - 26, kind: "top" as const };
  if (best.kind === "top") usedTop.set(spanKey, topSlot + 1);
  if (best.kind === "bot") usedBot.set(spanKey, botSlot + 1);
  return best.y;
}

export function layoutPatch(spec: PatchGraphSpec): PatchLayout {
  const columns = assignColumns(spec);
  const byCol = new Map<number, PatchNodeSpec[]>();
  for (const node of spec.nodes) {
    const col = columns.get(node.id) ?? 0;
    const list = byCol.get(col) ?? [];
    list.push(node);
    byCol.set(col, list);
  }

  const colKeys = [...byCol.keys()].sort((a, b) => a - b);
  orderColumns(byCol, colKeys, spec.edges);

  const PAD_X = 24;
  const PAD_Y = 24;
  const COL_GAP = 104;
  const ROW_GAP = 30;
  const NODE_H_BASE = 46;
  const NODE_H_SUB = 60;

  const colWidths = new Map<number, number>();
  const colHeights = new Map<number, number>();
  for (const col of colKeys) {
    const list = byCol.get(col) ?? [];
    let maxW = 140;
    let totalH = 0;
    for (const node of list) {
      const textLen = Math.max(node.label.length * 7.6 + 34, (node.sub?.length ?? 0) * 6.5 + 24);
      maxW = Math.max(maxW, Math.min(220, Math.ceil(textLen)));
      totalH += node.sub ? NODE_H_SUB : NODE_H_BASE;
    }
    totalH += Math.max(0, list.length - 1) * ROW_GAP;
    colWidths.set(col, maxW);
    colHeights.set(col, totalH);
  }

  const maxColH = Math.max(...colHeights.values(), NODE_H_SUB);
  const placed = new Map<string, PlacedNode>();
  let cursorX = PAD_X;

  for (const col of colKeys) {
    const list = byCol.get(col) ?? [];
    const colW = colWidths.get(col) ?? 150;
    const colH = colHeights.get(col) ?? NODE_H_BASE;
    let cursorY = PAD_Y + Math.round((maxColH - colH) / 2);

    for (let row = 0; row < list.length; row++) {
      const node = list[row];
      if (!node) continue;
      const h = node.sub ? NODE_H_SUB : NODE_H_BASE;
      placed.set(node.id, { ...node, col, row, x: cursorX, y: cursorY, w: colW, h });
      cursorY += h + ROW_GAP;
    }
    cursorX += colW + COL_GAP;
  }

  const outGroups = new Map<string, PatchEdgeSpec[]>();
  const inGroups = new Map<string, PatchEdgeSpec[]>();
  for (const edge of spec.edges) {
    const outList = outGroups.get(edge.from) ?? [];
    outList.push(edge);
    outGroups.set(edge.from, outList);

    const inList = inGroups.get(edge.to) ?? [];
    inList.push(edge);
    inGroups.set(edge.to, inList);
  }

  for (const list of outGroups.values()) {
    list.sort((a, b) => {
      const ta = placed.get(a.to);
      const tb = placed.get(b.to);
      return (ta ? ta.y + ta.h / 2 : 0) - (tb ? tb.y + tb.h / 2 : 0);
    });
  }
  for (const list of inGroups.values()) {
    list.sort((a, b) => {
      const fa = placed.get(a.from);
      const fb = placed.get(b.from);
      return (fa ? fa.y + fa.h / 2 : 0) - (fb ? fb.y + fb.h / 2 : 0);
    });
  }

  const portY = (node: PlacedNode, index: number, total: number): number => {
    const top = node.y + 26;
    const span = node.h - 32;
    if (total <= 1) return top + span / 2;
    return top + (span * (index + 0.5)) / total;
  };

  const allNodes = [...placed.values()];
  const nodeRects: Rect[] = allNodes.map((n) => ({ x: n.x, y: n.y, w: n.w, h: n.h }));
  const placedTags: Rect[] = [];
  const usedTop = new Map<string, number>();
  const usedBot = new Map<string, number>();

  const wires: PatchLayout["wires"] = [];
  const tags: PatchLayout["tags"] = [];
  let minY = PAD_Y;
  let maxY = maxColH + PAD_Y;

  for (const edge of spec.edges) {
    const from = placed.get(edge.from);
    const to = placed.get(edge.to);
    if (!from || !to) continue;

    const outList = outGroups.get(edge.from) ?? [edge];
    const inList = inGroups.get(edge.to) ?? [edge];
    const x1 = from.x + from.w;
    const y1 = portY(from, outList.indexOf(edge), outList.length);
    const x2 = to.x;
    const y2 = portY(to, inList.indexOf(edge), inList.length);
    const tag = edge.label ?? edge.port;
    const pillW = Math.max(28, tag.length * 6.4 + 14);
    const pillH = 18;

    const midNodes = allNodes.filter((n) => n.col > from.col && n.col < to.col);
    let path: string;
    const candidates: { x: number; y: number }[] = [];

    if (midNodes.length > 0) {
      const spanKey = `${from.col}-${to.col}`;
      const cy = pickCorridorY(midNodes, (y1 + y2) / 2, usedTop, usedBot, spanKey);
      const r = 26;
      const sx = x1 + r * 1.4;
      const ex = x2 - r * 1.4;
      path =
        `M${x1} ${y1} ` +
        `C${x1 + r} ${y1} ${x1 + r * 0.6} ${cy} ${sx} ${cy} ` +
        `L${ex} ${cy} ` +
        `C${x2 - r * 0.6} ${cy} ${x2 - r} ${y2} ${x2} ${y2}`;
      minY = Math.min(minY, cy - 14);
      maxY = Math.max(maxY, cy + 14);
      for (const frac of [0.5, 0.36, 0.64, 0.24, 0.76, 0.16, 0.84]) {
        candidates.push({ x: sx + (ex - sx) * frac, y: cy });
      }
    } else {
      const dx = Math.max(36, Math.abs(x2 - x1) * 0.48);
      path = `M${x1} ${y1} C${x1 + dx} ${y1} ${x2 - dx} ${y2} ${x2} ${y2}`;
      for (const t of [0.5, 0.42, 0.58, 0.34, 0.66, 0.26, 0.74]) {
        candidates.push(cubicPoint(x1, y1, x1 + dx, y1, x2 - dx, y2, x2, y2, t));
      }
    }

    let chosen = candidates[0] ?? { x: (x1 + x2) / 2, y: (y1 + y2) / 2 };
    for (const pt of candidates) {
      const box: Rect = { x: pt.x - pillW / 2, y: pt.y - pillH / 2, w: pillW, h: pillH };
      const hitsNode = nodeRects.some((r) => rectsOverlap(box, r, 3));
      const hitsTag = placedTags.some((r) => rectsOverlap(box, r, 3));
      if (!hitsNode && !hitsTag) {
        chosen = pt;
        break;
      }
    }

    const rect: Rect = { x: chosen.x - pillW / 2, y: chosen.y - pillH / 2, w: pillW, h: pillH };
    placedTags.push(rect);
    minY = Math.min(minY, rect.y);
    maxY = Math.max(maxY, rect.y + rect.h);
    wires.push({ edge, path, x1, y1, x2, y2 });
    tags.push({ edge, text: tag, cx: chosen.x, cy: chosen.y, rect });
  }

  const shiftY = minY < PAD_Y ? Math.ceil(PAD_Y - minY) : 0;
  if (shiftY > 0) {
    for (const n of allNodes) n.y += shiftY;
    for (const w of wires) {
      w.y1 += shiftY;
      w.y2 += shiftY;
      w.path = shiftPathY(w.path, shiftY);
    }
    for (const t of tags) {
      t.cy += shiftY;
      t.rect.y += shiftY;
    }
    maxY += shiftY;
  }

  return {
    width: cursorX - COL_GAP + PAD_X,
    height: Math.ceil(maxY + PAD_Y),
    nodes: allNodes,
    wires,
    tags,
  };
}

function shiftPathY(d: string, dy: number): string {
  return d
    .replace(/([MLC])([^MLC]+)/g, (_, cmd: string, args: string) => {
      const nums = args.trim().split(/\s+/).map(Number);
      for (let i = 1; i < nums.length; i += 2) {
        nums[i] = Number(((nums[i] ?? 0) + dy).toFixed(2));
      }
      return `${cmd}${nums.join(" ")} `;
    })
    .trim();
}

export function renderPatchSvg(source: string): string {
  const spec = parsePatch(source);
  if (spec.nodes.length === 0) {
    return "";
  }

  const layout = layoutPatch(spec);
  const wireSvg: string[] = [];
  const portSvg: string[] = [];
  const labelSvg: string[] = [];
  const ariaParts: string[] = [];

  for (const wire of layout.wires) {
    wireSvg.push(`<path class="pd-wire pd-port-${wire.edge.port}" d="${wire.path}" />`);
    const glyph = PORT_GLYPHS[wire.edge.port];
    portSvg.push(
      `<path class="pd-dot pd-port-${wire.edge.port}" d="${glyph}" transform="translate(${wire.x1} ${wire.y1})" />`,
      `<path class="pd-dot pd-port-${wire.edge.port}" d="${glyph}" transform="translate(${wire.x2} ${wire.y2})" />`,
    );
  }

  for (const tag of layout.tags) {
    const pillW = tag.rect.w;
    labelSvg.push(
      `<g class="pd-tag pd-port-${tag.edge.port}" transform="translate(${tag.cx.toFixed(1)} ${tag.cy.toFixed(1)})">` +
        `<rect x="${(-pillW / 2).toFixed(1)}" y="-9" width="${pillW.toFixed(1)}" height="18" rx="9" />` +
        `<text x="0" y="3.5" text-anchor="middle">${escapeHtml(tag.text)}</text>` +
        `</g>`,
    );
    ariaParts.push(`${tag.edge.from} ${tag.text} to ${tag.edge.to}`);
  }

  const nodeSvg: string[] = [];
  for (const node of layout.nodes) {
    const headPath = `M0 22V5A5 5 0 0 1 5 0H${node.w - 5}A5 5 0 0 1 ${node.w} 5V22Z`;
    nodeSvg.push(
      `<g class="pd-node pd-cat-${node.category}" transform="translate(${node.x} ${node.y})">` +
        `<rect class="pd-face" width="${node.w}" height="${node.h}" rx="5" />` +
        `<path class="pd-head" d="${headPath}" />` +
        `<line class="pd-rule" x1="0" y1="22" x2="${node.w}" y2="22" />` +
        `<circle class="pd-cat" cx="11" cy="11" r="3" />` +
        `<text class="pd-title" x="20" y="14.5">${escapeHtml(node.label)}</text>` +
        (node.sub
          ? `<text class="pd-sub" x="11" y="44">${escapeHtml(node.sub)}</text>`
          : `<text class="pd-kind" x="11" y="38">${escapeHtml(node.category)}</text>`) +
        `</g>`,
    );
  }

  const aria = escapeHtml(
    ariaParts.length > 0 ? `Patch diagram: ${ariaParts.join("; ")}.` : "Patch diagram.",
  );

  return (
    `<figure class="patch-diagram">` +
    `<svg viewBox="0 0 ${layout.width} ${layout.height}" width="${layout.width}" height="${layout.height}" role="img" aria-label="${aria}">` +
    `<g class="pd-wires">${wireSvg.join("")}</g>` +
    `<g class="pd-nodes">${nodeSvg.join("")}</g>` +
    `<g class="pd-ports">${portSvg.join("")}</g>` +
    `<g class="pd-tags">${labelSvg.join("")}</g>` +
    `</svg>` +
    `</figure>`
  );
}
