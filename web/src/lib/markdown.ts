export type Span =
  | { kind: "text"; text: string }
  | { kind: "strong"; text: string }
  | { kind: "em"; text: string }
  | { kind: "code"; text: string }
  | { kind: "link"; text: string; href: string };

export type Block =
  | { kind: "heading"; level: 1 | 2 | 3; spans: Span[] }
  | { kind: "paragraph"; spans: Span[] }
  | { kind: "list"; ordered: boolean; items: Span[][] }
  | { kind: "quote"; spans: Span[] }
  | { kind: "code"; text: string }
  | { kind: "rule" };

const WEB = /^(?:https?:\/\/|mailto:)/i;
const INLINE = /`([^`]+)`|\*\*(.+?)\*\*|\*(.+?)\*|(?<!\w)_(.+?)_(?!\w)|\[([^\]]+)\]\(([^)\s]+)\)/g;
const FENCE = /^```/;
const RULE = /^(?:-{3,}|\*{3,}|_{3,})$/;
const HEADING = /^(#{1,6})\s+(.*)$/;
const BULLET = /^[-*+]\s+(.*)$/;
const NUMBER = /^\d+[.)]\s+(.*)$/;
const QUOTE = /^>\s?(.*)$/;

export function markdownHref(target: string): string | null {
  return WEB.test(target) ? target : null;
}

export function markdownSpans(line: string): Span[] {
  const spans: Span[] = [];
  let plain = 0;
  for (const match of line.matchAll(INLINE)) {
    const at = match.index ?? 0;
    addText(spans, line.slice(plain, at));
    const marked = markedSpan(match);
    if (typeof marked === "string") {
      addText(spans, marked);
    } else {
      spans.push(marked);
    }
    plain = at + match[0].length;
  }
  addText(spans, line.slice(plain));
  return spans;
}

export function markdownBlocks(text: string): Block[] {
  const lines = text.replaceAll("\r", "").split("\n");
  const blocks: Block[] = [];
  let index = 0;
  while (index < lines.length) {
    index = readBlock(lines, index, blocks);
  }
  return blocks;
}

function markedSpan(match: RegExpMatchArray): Span | string {
  const [, code, strong, star, under, label, target] = match;
  if (code !== undefined) {
    return { kind: "code", text: code };
  }
  if (strong !== undefined) {
    return { kind: "strong", text: strong };
  }
  const italic = star ?? under;
  if (italic !== undefined) {
    return { kind: "em", text: italic };
  }
  if (label === undefined || target === undefined) {
    return match[0];
  }
  const href = markdownHref(target);
  return href === null ? label : { kind: "link", text: label, href };
}

function addText(spans: Span[], text: string): void {
  if (text === "") {
    return;
  }
  const last = spans.at(-1);
  if (last?.kind === "text") {
    last.text += text;
  } else {
    spans.push({ kind: "text", text });
  }
}

function readBlock(lines: readonly string[], start: number, blocks: Block[]): number {
  const line = lines[start]?.trim() ?? "";
  if (line === "") {
    return start + 1;
  }
  if (FENCE.test(line)) {
    return readFence(lines, start, blocks);
  }
  if (RULE.test(line)) {
    blocks.push({ kind: "rule" });
    return start + 1;
  }
  const heading = HEADING.exec(line);
  if (heading !== null) {
    blocks.push({
      kind: "heading",
      level: headingLevel(heading[1] ?? "#"),
      spans: markdownSpans(heading[2] ?? ""),
    });
    return start + 1;
  }
  if (BULLET.test(line) || NUMBER.test(line)) {
    return readList(lines, start, blocks);
  }
  if (QUOTE.test(line)) {
    return readQuote(lines, start, blocks);
  }
  return readParagraph(lines, start, blocks);
}

function headingLevel(hashes: string): 1 | 2 | 3 {
  return hashes.length === 1 ? 1 : hashes.length === 2 ? 2 : 3;
}

function readFence(lines: readonly string[], start: number, blocks: Block[]): number {
  const body: string[] = [];
  let index = start + 1;
  while (index < lines.length && !FENCE.test(lines[index]?.trim() ?? "")) {
    body.push(lines[index] ?? "");
    index += 1;
  }
  blocks.push({ kind: "code", text: body.join("\n") });
  return index + 1;
}

function readList(lines: readonly string[], start: number, blocks: Block[]): number {
  const ordered = NUMBER.test(lines[start]?.trim() ?? "");
  const marker = ordered ? NUMBER : BULLET;
  const items: Span[][] = [];
  let index = start;
  while (index < lines.length) {
    const item = marker.exec(lines[index]?.trim() ?? "");
    if (item === null) {
      break;
    }
    items.push(markdownSpans(item[1] ?? ""));
    index += 1;
  }
  blocks.push({ kind: "list", ordered, items });
  return index;
}

function readQuote(lines: readonly string[], start: number, blocks: Block[]): number {
  const body: string[] = [];
  let index = start;
  while (index < lines.length) {
    const quoted = QUOTE.exec(lines[index]?.trim() ?? "");
    if (quoted === null) {
      break;
    }
    body.push(quoted[1] ?? "");
    index += 1;
  }
  blocks.push({ kind: "quote", spans: markdownSpans(body.join(" ").trim()) });
  return index;
}

function readParagraph(lines: readonly string[], start: number, blocks: Block[]): number {
  const body: string[] = [];
  let index = start;
  while (index < lines.length) {
    const line = lines[index]?.trim() ?? "";
    if (line === "" || opensBlock(line)) {
      break;
    }
    body.push(line);
    index += 1;
  }
  blocks.push({ kind: "paragraph", spans: markdownSpans(body.join(" ")) });
  return index;
}

function opensBlock(line: string): boolean {
  return (
    FENCE.test(line) ||
    RULE.test(line) ||
    HEADING.test(line) ||
    BULLET.test(line) ||
    NUMBER.test(line) ||
    QUOTE.test(line)
  );
}
