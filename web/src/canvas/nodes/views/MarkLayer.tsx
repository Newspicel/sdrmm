import type { ViewMark } from "../../../lib/types";

const DOT_R = 4;
const CROSS = 4;
const SQUARE_HALF = 4;
const TRIANGLE_R = 5;

export type Place = (x: number, y: number) => { x: number; y: number };

/** Marks a processor puts on its surface, placed by the surface's own axes. */
export function MarkLayer({
  marks,
  at,
  picked,
}: {
  marks: readonly ViewMark[];
  at: Place;
  picked: number | null;
}) {
  return (
    <>
      {marks.map((mark, index) => (
        <MarkGlyph
          key={mark.key ?? `m${index}`}
          mark={mark}
          at={at}
          picked={mark.key != null && mark.key === picked}
        />
      ))}
    </>
  );
}

function MarkGlyph({ mark, at, picked }: { mark: ViewMark; at: Place; picked: boolean }) {
  const { x, y } = at(mark.x, mark.y);
  const title = mark.title == null ? null : <title>{mark.title}</title>;
  switch (mark.shape) {
    case "dot":
      return <circle cx={x} cy={y} r={DOT_R} className="fill-none stroke-plot-ink/60" />;
    case "cross":
      return (
        <path
          d={`M${x - CROSS} ${y - CROSS}L${x + CROSS} ${y + CROSS}M${x - CROSS} ${y + CROSS}L${x + CROSS} ${y - CROSS}`}
          className="stroke-plot-ink"
          strokeWidth={1.5}
        >
          {title}
        </path>
      );
    case "triangle":
      return (
        <path
          d={`M${x} ${y - TRIANGLE_R}L${x + TRIANGLE_R} ${y + TRIANGLE_R}L${x - TRIANGLE_R} ${y + TRIANGLE_R}Z`}
          className="fill-none stroke-hue-amber"
        >
          {title}
        </path>
      );
    case "square":
      return <SquareMark mark={mark} at={at} head={{ x, y }} picked={picked} />;
  }
}

function SquareMark({
  mark,
  at,
  head,
  picked,
}: {
  mark: ViewMark;
  at: Place;
  head: { x: number; y: number };
  picked: boolean;
}) {
  const trail = (mark.trail ?? [])
    .map(([tx = 0, ty = 0]) => at(tx, ty))
    .map((point) => `${point.x.toFixed(1)},${point.y.toFixed(1)}`)
    .join(" ");
  return (
    <g className="stroke-accent">
      {(mark.trail?.length ?? 0) > 1 && (
        <polyline points={trail} className="fill-none opacity-60" strokeWidth={1} />
      )}
      <rect
        x={head.x - SQUARE_HALF}
        y={head.y - SQUARE_HALF}
        width={SQUARE_HALF * 2}
        height={SQUARE_HALF * 2}
        className="fill-none"
        strokeWidth={picked ? 2.5 : 1.25}
        strokeDasharray={mark.faint === true ? "2 2" : undefined}
      />
      {mark.label != null && (
        <text
          x={head.x + SQUARE_HALF + 2}
          y={head.y - SQUARE_HALF}
          className="fill-accent stroke-none font-mono text-plot"
        >
          {mark.label}
        </text>
      )}
    </g>
  );
}
