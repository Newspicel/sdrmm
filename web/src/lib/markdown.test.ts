import { describe, expect, it } from "vitest";
import { markdownBlocks, markdownHref, markdownSpans } from "./markdown";

describe("markdownSpans", () => {
  it("keeps plain text in one span", () => {
    expect(markdownSpans("tune 145.800")).toEqual([{ kind: "text", text: "tune 145.800" }]);
  });

  it("reads bold, italic and code", () => {
    expect(markdownSpans("**wide** _then_ `narrow`")).toEqual([
      { kind: "strong", text: "wide" },
      { kind: "text", text: " " },
      { kind: "em", text: "then" },
      { kind: "text", text: " " },
      { kind: "code", text: "narrow" },
    ]);
  });

  it("leaves markers inside code alone", () => {
    expect(markdownSpans("`**raw**`")).toEqual([{ kind: "code", text: "**raw**" }]);
  });

  it("reads a link", () => {
    expect(markdownSpans("see [the docs](https://sdrmm.app)")).toEqual([
      { kind: "text", text: "see " },
      { kind: "link", text: "the docs", href: "https://sdrmm.app" },
    ]);
  });

  it("drops the link but keeps the words when the target is not a web address", () => {
    expect(markdownSpans("[run](javascript:alert)")).toEqual([{ kind: "text", text: "run" }]);
  });
});

describe("markdownHref", () => {
  it("takes web and mail addresses", () => {
    expect(markdownHref("https://sdrmm.app")).toBe("https://sdrmm.app");
    expect(markdownHref("http://sdrmm.app")).toBe("http://sdrmm.app");
    expect(markdownHref("mailto:someone@example.com")).toBe("mailto:someone@example.com");
  });

  it("refuses everything else", () => {
    expect(markdownHref("javascript:alert(1)")).toBeNull();
    expect(markdownHref("data:text/html,<script>")).toBeNull();
    expect(markdownHref("/local/path")).toBeNull();
  });
});

describe("markdownBlocks", () => {
  it("reads nothing out of blank text", () => {
    expect(markdownBlocks("   \n\n ")).toEqual([]);
  });

  it("reads headings down to three levels", () => {
    expect(markdownBlocks("# One\n## Two\n### Three\n#### Four")).toEqual([
      { kind: "heading", level: 1, spans: [{ kind: "text", text: "One" }] },
      { kind: "heading", level: 2, spans: [{ kind: "text", text: "Two" }] },
      { kind: "heading", level: 3, spans: [{ kind: "text", text: "Three" }] },
      { kind: "heading", level: 3, spans: [{ kind: "text", text: "Four" }] },
    ]);
  });

  it("joins the lines of one paragraph and splits on a blank line", () => {
    expect(markdownBlocks("first line\nsame paragraph\n\nsecond")).toEqual([
      { kind: "paragraph", spans: [{ kind: "text", text: "first line same paragraph" }] },
      { kind: "paragraph", spans: [{ kind: "text", text: "second" }] },
    ]);
  });

  it("gathers bullets into one list", () => {
    expect(markdownBlocks("- one\n* two")).toEqual([
      {
        kind: "list",
        ordered: false,
        items: [[{ kind: "text", text: "one" }], [{ kind: "text", text: "two" }]],
      },
    ]);
  });

  it("gathers numbers into an ordered list", () => {
    expect(markdownBlocks("1. first\n2. second")).toEqual([
      {
        kind: "list",
        ordered: true,
        items: [[{ kind: "text", text: "first" }], [{ kind: "text", text: "second" }]],
      },
    ]);
  });

  it("keeps a fenced block verbatim", () => {
    expect(markdownBlocks("```\n- not a list\n**raw**\n```")).toEqual([
      { kind: "code", text: "- not a list\n**raw**" },
    ]);
  });

  it("reads a quote and a rule", () => {
    expect(markdownBlocks("> watch the squelch\n\n---")).toEqual([
      { kind: "quote", spans: [{ kind: "text", text: "watch the squelch" }] },
      { kind: "rule" },
    ]);
  });

  it("ignores carriage returns", () => {
    expect(markdownBlocks("# One\r\nbody\r\n")).toEqual([
      { kind: "heading", level: 1, spans: [{ kind: "text", text: "One" }] },
      { kind: "paragraph", spans: [{ kind: "text", text: "body" }] },
    ]);
  });
});
