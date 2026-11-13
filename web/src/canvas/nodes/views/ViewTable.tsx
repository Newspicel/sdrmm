import { Button } from "../../../components/BaseControls";
import { TABLE_CELL, TABLE_HEAD } from "../../../components/controls";
import type { TableView } from "../../../lib/types";

type Row = TableView["rows"][number];
type Cell = Row["cells"][number];

export function ViewTable({
  view,
  picked,
  onPick,
}: {
  view: TableView | null;
  picked: number | null;
  onPick: (key: number | null) => void;
}) {
  if (view == null || view.rows.length === 0) {
    return null;
  }
  return (
    <table className="w-full shrink-0 border-t border-line">
      <thead>
        <tr>
          {view.heads.map((head) => (
            <th key={head} className={TABLE_HEAD}>
              {head}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {view.rows.map((row) => (
          <ViewRow key={row.key} row={row} chosen={row.key === picked} onPick={onPick} />
        ))}
      </tbody>
    </table>
  );
}

function ViewRow({
  row,
  chosen,
  onPick,
}: {
  row: Row;
  chosen: boolean;
  onPick: (key: number | null) => void;
}) {
  const [first, ...rest] = row.cells;
  const tone = row.pick === true ? (chosen ? "bg-accent/15" : "hover:bg-panel-2") : "";
  return (
    <tr
      className={`${tone} ${row.dim === true ? "opacity-45" : ""}`}
      title={row.pick === true ? undefined : (row.title ?? undefined)}
    >
      {first !== undefined && (
        <td className={TABLE_CELL}>
          {row.pick === true ? (
            <Button
              type="button"
              aria-pressed={chosen}
              title={row.title ?? undefined}
              className="cursor-pointer font-mono hover:text-accent"
              onClick={() => onPick(chosen ? null : row.key)}
            >
              {first.text}
            </Button>
          ) : (
            first.text
          )}
        </td>
      )}
      {rest.map((cell, index) => (
        <ViewCell key={index} cell={cell} />
      ))}
    </tr>
  );
}

function ViewCell({ cell }: { cell: Cell }) {
  return (
    <td
      className={`${TABLE_CELL} max-w-20 truncate ${cell.faint === true ? "text-ink-faint" : ""}`}
      title={cell.title ?? undefined}
    >
      {cell.text}
    </td>
  );
}
