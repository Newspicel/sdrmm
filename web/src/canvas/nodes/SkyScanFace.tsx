import { Button } from "../../components/BaseControls";
import { FACE_BTN, FACE_BTN_DANGER } from "../../components/controls";
import { useSkyScanControl, useSkyScanStore } from "../../lib/pointing";
import type { Field, PatchNode, SkyCell, SkyScanParams, SkyScanStatus } from "../../lib/types";
import { useWorkspaceSelect } from "../context";
import { NodeForm, nodeSettings, StatusGrid } from "./NodeForm";
import { FaceBody, FaceFooter, FaceStanding, NodeShell } from "./NodeShell";
import { useUnwired, type Want } from "./needs";
import { heatColor, levelSpan, skySlot } from "./skyScan";

const NO_READOUTS: readonly Field[] = [];
const NO_CELLS: readonly SkyCell[] = [];
const CELL_PX = 12;
const WANTS: readonly Want[] = [{ port: "iq" }, { port: "control", out: true }];

export function SkyScanFace({ node }: { node: PatchNode }) {
  const catalog = useWorkspaceSelect((workspace) => workspace.context.catalog);
  const status = useSkyScanStore((store) => store.byNode[node.id]);
  const { control, pending } = useSkyScanControl(node.id);
  const unwired = useUnwired(node.id, WANTS);
  if (node.kind !== "sky_scan") {
    return null;
  }
  const settings = nodeSettings(catalog, node) as SkyScanParams | null;
  const running = status?.running === true;
  const cells = status?.cells ?? NO_CELLS;
  return (
    <NodeShell node={node} title="Sky scan" category="tool" live={running}>
      <FaceBody>
        {settings !== null && status !== undefined && (
          <SkyHeat settings={settings} status={status} cells={cells} />
        )}
        <NodeForm node={node} />
        <StatusGrid readouts={status?.readouts ?? NO_READOUTS} />
        <FaceStanding unwired={unwired} error={status?.error} waiting={status?.waiting} />
      </FaceBody>
      <FaceFooter>
        <Button
          className={FACE_BTN_DANGER}
          type="button"
          title="Forget every measured point"
          disabled={pending || cells.length === 0}
          onClick={() => control("clear")}
        >
          Clear
        </Button>
        <Button
          className={running ? FACE_BTN_DANGER : FACE_BTN}
          type="button"
          disabled={pending || (!running && unwired)}
          onClick={() => control(running ? "stop" : "start")}
        >
          {running ? "Stop" : "Start"}
        </Button>
      </FaceFooter>
    </NodeShell>
  );
}

/** How much wider than tall a one-row scan draws: a strip of cells, not a tall box. */
const FLAT = 12;

/** Every cell the scan will measure, so an empty map shows its grid. */
function slots(columns: number, rows: number): { column: number; row: number }[] {
  return Array.from({ length: columns * rows }, (_, index) => ({
    column: index % columns,
    row: Math.floor(index / columns),
  }));
}

function SkyHeat({
  settings,
  status,
  cells,
}: {
  settings: SkyScanParams;
  status: SkyScanStatus;
  cells: readonly SkyCell[];
}) {
  const columns = Math.max(status.columns, 1);
  const rows = Math.max(status.rows, 1);
  const span = levelSpan(cells);
  return (
    <div className="shrink-0 border-b border-line">
      <svg
        role="img"
        aria-label="Level by bearing and elevation"
        className="block w-full bg-ink/5"
        viewBox={`0 0 ${columns * CELL_PX} ${rows * CELL_PX}`}
        preserveAspectRatio="none"
        style={{ aspectRatio: `${columns} / ${Math.max(rows, columns / FLAT)}` }}
      >
        {slots(columns, rows).map(({ column, row }) => (
          <rect
            key={`slot:${column}:${row}`}
            x={column * CELL_PX}
            y={row * CELL_PX}
            width={CELL_PX}
            height={CELL_PX}
            fill="none"
            className="stroke-line"
            vectorEffect="non-scaling-stroke"
          />
        ))}
        {span !== null &&
          cells.map((cell) => {
            const slot = skySlot(settings, cell);
            return (
              <rect
                key={`${slot.column}:${slot.row}`}
                x={slot.column * CELL_PX}
                y={(rows - 1 - slot.row) * CELL_PX}
                width={CELL_PX}
                height={CELL_PX}
                fill={heatColor(cell.level_dbfs, span)}
              >
                <title>
                  {`${cell.azimuth_deg.toFixed(0)}° ${cell.elevation_deg.toFixed(0)}°: ${cell.level_dbfs.toFixed(1)} dBFS`}
                </title>
              </rect>
            );
          })}
      </svg>
    </div>
  );
}
