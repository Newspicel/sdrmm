import { Button } from "../../components/BaseControls";
import { BTN, TABLE_CELL, TABLE_HEAD } from "../../components/controls";
import { FaceStats, Stat } from "../../components/face/Stats";
import { formatCount } from "../../components/format";
import { useFusionClear, useFusionSeed, useFusionStore } from "../../lib/fusion";
import type { DfFusionState, DfStation, Field, PatchNode } from "../../lib/types";
import { useNow } from "../../lib/useNow";
import { useWorkspaceContext } from "../context";
import { FusionHeat } from "./FusionHeat";
import { NodeForm, StatusGrid } from "./NodeForm";
import { FaceBody, FaceFooter, NodeShell } from "./NodeShell";
import {
  fusionSources,
  NO_SOURCES,
  stationAge,
  stationAlign,
  stationBearing,
  stationSigma,
} from "./triangulation";

const AGE_TICK_MS = 1_000;
const STATION_HEADS = ["Station", "Last", "±", "Align", "Age"] as const;
const NO_READOUTS: readonly Field[] = [];

export function TriangulationFace({ node }: { node: PatchNode }) {
  const workspace = useWorkspaceContext();
  const fusion = useFusionStore((store) => store.byNode[node.id]);
  const readouts = useFusionStore((store) => store.statusByNode[node.id]?.readouts ?? NO_READOUTS);
  useFusionSeed(node.id);
  const { clear, pending } = useFusionClear(node.id);
  const now = useNow(AGE_TICK_MS);
  if (node.kind !== "triangulation") {
    return null;
  }
  const stations = fusion?.stations ?? [];
  const sources = fusionSources(workspace.resolved, node.id);
  return (
    <NodeShell
      node={node}
      title="Triangulation"
      category="tool"
      subtitle={`${stations.length} of ${sources} reporting`}
    >
      <FaceBody>
        <FusionHeat
          node={node.id}
          known={fusion !== undefined}
          estimate={fusion?.estimate ?? null}
          emitters={fusion?.emitters ?? []}
          stations={stations}
          hint={sources === 0 ? NO_SOURCES : null}
        />
        <NodeForm node={node} />
        <StatusGrid readouts={readouts} columns={1} />
        <StationTable stations={stations} now={now} />
      </FaceBody>
      <FaceFooter>
        <FusionStats fusion={fusion} />
        <Button
          className={BTN}
          type="button"
          title="Throw away every bearing and start again"
          disabled={pending}
          onClick={clear}
        >
          Clear
        </Button>
      </FaceFooter>
    </NodeShell>
  );
}

function FusionStats({ fusion }: { fusion: DfFusionState | undefined }) {
  const dropped = fusion?.dropped ?? 0;
  const refused = fusion?.refused ?? 0;
  return (
    <FaceStats>
      {dropped > 0 && (
        <Stat label="Dropped" title="Bearings lost, the queue was full" tone="danger">
          {formatCount(dropped)}
        </Stat>
      )}
      {refused > 0 && (
        <Stat label="Refused" title="Bearings without a place or below Min conf" tone="danger">
          {formatCount(refused)}
        </Stat>
      )}
    </FaceStats>
  );
}

function StationTable({ stations, now }: { stations: readonly DfStation[]; now: number }) {
  if (stations.length === 0) {
    return null;
  }
  return (
    <table className="w-full shrink-0 border-t border-line" aria-label="Stations">
      <thead>
        <tr>
          {STATION_HEADS.map((head) => (
            <th key={head} className={TABLE_HEAD}>
              {head}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {stations.map((station) => (
          <tr key={station.station_id}>
            <td className={`${TABLE_CELL} max-w-24 truncate`} title={station.station_id}>
              {station.station_id}
            </td>
            <td className={TABLE_CELL}>{stationBearing(station)}</td>
            <td className={TABLE_CELL}>{stationSigma(station)}</td>
            <td className={TABLE_CELL} title="Heading offset learned from crossing bearings">
              {stationAlign(station)}
            </td>
            <td className={TABLE_CELL}>{stationAge(station, now)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
