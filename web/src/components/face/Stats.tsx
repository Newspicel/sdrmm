import type { ReactNode } from "react";
import type { ChipTone } from "./Chips";

const TONE: Record<ChipTone, string> = {
  ok: "text-ok",
  warn: "text-warn",
  danger: "text-danger",
};

export function FaceStats({ children }: { children: ReactNode }) {
  return (
    <span className="flex min-w-0 flex-[1_1_10rem] flex-wrap items-center gap-3">{children}</span>
  );
}

export function Stat({
  label,
  title,
  tone,
  children,
}: {
  label: ReactNode;
  title: string;
  tone?: ChipTone;
  children?: ReactNode;
}) {
  const shade = tone !== undefined ? TONE[tone] : "text-ink";
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
