import type { ReactNode } from "react";
import type { ChipTone } from "./Chips";

const TONE: Record<ChipTone, string> = {
  ok: "text-ok",
  warn: "text-warn",
  danger: "text-danger",
};

export function FaceStats({ children }: { children: ReactNode }) {
  return <span className="mr-auto flex min-w-0 flex-wrap items-center gap-3">{children}</span>;
}

export function Stat({
  label,
  title,
  tone,
  quiet = false,
  children,
}: {
  label: ReactNode;
  title: string;
  tone?: ChipTone;
  quiet?: boolean;
  children?: ReactNode;
}) {
  const shade = tone !== undefined ? TONE[tone] : quiet ? "text-ink-faint" : "text-ink";
  return (
    <span
      className={`inline-flex items-center gap-1 font-mono text-2xs whitespace-nowrap ${
        children === undefined && tone !== undefined ? TONE[tone] : "text-ink-faint"
      }`}
      title={title}
    >
      {label}
      {children !== undefined && <b className={`font-medium ${shade}`}>{children}</b>}
    </span>
  );
}
