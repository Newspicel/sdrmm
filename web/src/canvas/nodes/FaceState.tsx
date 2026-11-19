import type { ReactNode } from "react";

/**
 * The top row of a signal face where a decoder has its dial. What the node still needs reads
 * as the same faint hint every face uses; once it has it, the row carries its values.
 */
export function FaceState({ need, children }: { need: string | null; children?: ReactNode }) {
  if (need !== null) {
    return (
      <div className="flex h-8.5 min-w-0 items-center justify-center px-1.5 text-xs text-ink-faint">
        <span className="truncate">{need}</span>
      </div>
    );
  }
  return (
    <div className="flex h-8.5 min-w-0 items-center gap-3 overflow-hidden px-1.5 text-xs">
      {children}
    </div>
  );
}

/** A small label and value in a face's state row. */
export function StateValue({ label, children }: { label: string; children: ReactNode }) {
  return (
    <span className="flex min-w-0 shrink-0 items-baseline gap-1.5">
      <span className="text-ink-dim">{label}</span>
      <span className="truncate font-mono text-ink">{children}</span>
    </span>
  );
}
