import type { CSSProperties, ReactNode } from "react";

/** The grid a plot draws before it has data, so an idle plot keeps its look. */
export const PLOT_GRID: CSSProperties = {
  backgroundImage:
    "linear-gradient(to right, var(--color-plot-grid) 1px, transparent 1px), " +
    "linear-gradient(to bottom, var(--color-plot-grid) 1px, transparent 1px)",
  backgroundSize: "12.5% 25%",
  backgroundPosition: "-1px -1px",
};

/** An empty plot: its ground and grid, and a faint hint when there is one. */
export function PlotIdle({
  hint,
  className = "",
  children,
}: {
  hint?: string | null;
  className?: string;
  children?: ReactNode;
}) {
  return (
    <div className={`relative bg-plot-bg ${className}`} style={PLOT_GRID}>
      {children}
      {hint != null && <PlotHint hint={hint} />}
    </div>
  );
}

/** The faint note an idle plot carries, centred on it. */
export function PlotHint({ hint }: { hint: string }) {
  return (
    <div className="pointer-events-none absolute inset-0 flex items-center justify-center p-2">
      <span className="rounded-inset bg-plot-bg/85 px-2 py-1 text-center text-xs text-balance text-plot-ink-dim">
        {hint}
      </span>
    </div>
  );
}
