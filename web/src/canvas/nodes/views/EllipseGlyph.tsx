import type { EllipseView } from "../../../lib/types";
import { ellipsePath, senseArrow } from "./ellipse";

const GLYPH_PX = 96;

export function EllipseGlyph({ view, dim }: { view: EllipseView | null; dim: boolean }) {
  const centre = GLYPH_PX / 2;
  const radius = centre - 8;
  const angle = view?.angle_deg ?? 0;
  const chi = view?.ellipticity_deg ?? 0;
  const arrow = view == null ? null : senseArrow(angle, chi, view.v, radius, centre);
  return (
    <div className={`flex justify-center ${dim ? "opacity-45" : ""}`}>
      <svg
        role="img"
        aria-label="Polarisation"
        width={GLYPH_PX}
        height={GLYPH_PX}
        viewBox={`0 0 ${GLYPH_PX} ${GLYPH_PX}`}
        className="my-2 shrink-0 rounded-control bg-well"
      >
        <path
          d={`M4 ${centre}H${GLYPH_PX - 4}M${centre} 4V${GLYPH_PX - 4}`}
          className="stroke-line"
        />
        <text
          x={GLYPH_PX - 4}
          y={centre - 3}
          textAnchor="end"
          className="fill-ink-faint font-mono text-plot"
        >
          H
        </text>
        <text x={centre + 3} y={10} className="fill-ink-faint font-mono text-plot">
          V
        </text>
        {view != null && (
          <>
            <path
              d={ellipsePath(angle, chi, radius, centre)}
              className="fill-accent/15 stroke-accent"
              strokeWidth={1.5}
            />
            {arrow !== null && (
              <>
                <path d={arrow.arc} className="fill-none stroke-accent" strokeWidth={2.5} />
                <path d={arrow.head} className="fill-accent stroke-accent" strokeLinejoin="round" />
              </>
            )}
          </>
        )}
      </svg>
    </div>
  );
}
