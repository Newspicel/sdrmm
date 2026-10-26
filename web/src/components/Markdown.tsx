import { type Block, markdownBlocks, type Span } from "../lib/markdown";

const HEADINGS = {
  1: "text-sm font-semibold text-ink",
  2: "text-xs font-semibold text-ink",
  3: "text-xs font-medium text-ink",
} as const;

const CODE = "rounded-inset bg-ink/5 px-1 font-mono text-2xs text-ink";

export function Markdown({ text }: { text: string }) {
  return (
    <div className="flex flex-col gap-1.5 px-2.5 py-2 text-xs leading-snug text-ink-dim">
      {markdownBlocks(text).map((block, index) => (
        <Piece key={index} block={block} />
      ))}
    </div>
  );
}

function Piece({ block }: { block: Block }) {
  switch (block.kind) {
    case "heading": {
      const Tag = `h${block.level}` as const;
      return (
        <Tag className={HEADINGS[block.level]}>
          <Marked spans={block.spans} />
        </Tag>
      );
    }
    case "paragraph":
      return (
        <p className="text-pretty">
          <Marked spans={block.spans} />
        </p>
      );
    case "list":
      return (
        <ListBlock ordered={block.ordered}>
          {block.items.map((item, index) => (
            <li key={index}>
              <Marked spans={item} />
            </li>
          ))}
        </ListBlock>
      );
    case "quote":
      return (
        <blockquote className="border-line-strong border-l-2 pl-2 text-ink-faint italic">
          <Marked spans={block.spans} />
        </blockquote>
      );
    case "code":
      return (
        <pre className="overflow-x-auto rounded-inset bg-ink/5 px-2 py-1 font-mono text-2xs text-ink">
          {block.text}
        </pre>
      );
    case "rule":
      return <hr className="border-line border-t" />;
  }
}

function ListBlock({ ordered, children }: { ordered: boolean; children: React.ReactNode }) {
  const style = "flex flex-col gap-0.5 pl-4";
  return ordered ? (
    <ol className={`${style} list-decimal`}>{children}</ol>
  ) : (
    <ul className={`${style} list-disc`}>{children}</ul>
  );
}

function Marked({ spans }: { spans: readonly Span[] }) {
  return spans.map((span, index) => <Inline key={index} span={span} />);
}

function Inline({ span }: { span: Span }) {
  switch (span.kind) {
    case "text":
      return span.text;
    case "strong":
      return <strong className="font-semibold text-ink">{span.text}</strong>;
    case "em":
      return <em className="italic">{span.text}</em>;
    case "code":
      return <code className={CODE}>{span.text}</code>;
    case "link":
      return (
        <a
          href={span.href}
          target="_blank"
          rel="noreferrer"
          className="text-accent underline underline-offset-2"
        >
          {span.text}
        </a>
      );
  }
}
