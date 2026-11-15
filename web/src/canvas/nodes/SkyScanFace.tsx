import { Button } from "../../components/BaseControls";
import { BTN, BTN_PRIMARY } from "../../components/controls";
import { FaceFault } from "../../components/face/Fault";
import { useSkyScanControl, useSkyScanStore } from "../../lib/pointing";
import type { Field, PatchNode, SkyCell, SkyScanParams, SkyScanStatus } from "../../lib/types";
import { useWorkspaceContext } from "../context";
import { NodeForm, nodeSettings, StatusGrid } from "./NodeForm";
import { FaceBody, FaceFooter, NodeShell } from "./NodeShell";
import { heatColor, levelSpan, skySlot } from "./skyScan";

const NO_READOUTS: readonly Field[] = [];
const NO_CELLS: readonly SkyCell[] = [];
const CELL_PX = 12;

export function SkyScanFace({ node }: { node: PatchNode }) {
  const workspace = useWorkspaceContext();
  const status = useSkyScanStore((store) => store.byNode[node.id]);
  const { control, pending } = useSkyScanControl(node.id);
  if (node.kind !== "sky_scan") {
    return null;
  }
  const settings = nodeSettings(workspace.context.catalog, node) as SkyScanParams | null;
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
        {status?.error != null && <FaceFault message={status.error} />}
      </FaceBody>
      <FaceFooter>
        <Button
          className={BTN}
          type="button"
          title="Forget every measured point"
          disabled={pending || cells.length === 0}
          onClick={() => control("clear")}
        >
          Clear
        </Button>
        <Button
          className={running ? BTN : BTN_PRIMARY}
          type="button"
          disabled={pending}
          onClick={() => control(running ? "stop" : "start")}
        >
          {running ? "Stop" : "Start"}
        </Button>
      </FaceFooter>
    </NodeShell>
  );
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
    <svg
      role="img"
      aria-label="Level by bearing and elevation"
      className="w-full shrink-0 border-b border-line bg-ink/5"
      viewBox={`0 0 ${columns * CELL_PX} ${rows * CELL_PX}`}
      preserveAspectRatio="none"
      style={{ aspectRatio: `${columns} / ${Math.max(rows, columns / 6)}` }}
    >
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
  );
}
