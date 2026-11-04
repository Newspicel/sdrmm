import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "../../components/BaseControls";
import { BTN, TABLE_CELL, TABLE_HEAD } from "../../components/controls";
import { ChoiceChip } from "../../components/face/Chips";
import { COLOUR_OPTIONS } from "../../components/plotFrame";
import { type Colormap, DEFAULT_COLORMAP } from "../../gl/surface";
import { clearRadarTracks } from "../../lib/api";
import { processorStatusOf, useArrayStore } from "../../lib/arrays";
import { readingOf, readoutsOf, useProcessorStore } from "../../lib/processors";
import { clearAction, failAction } from "../../lib/refusals";
import type { PassiveRadarParams, PatchNode, RadarUpdate } from "../../lib/types";
import { useNow } from "../../lib/useNow";
import { useWorkspaceContext } from "../context";
import { arrayOf, hasFeed } from "../lanes";
import { NodeForm, nodeSettings, StatusGrid } from "./NodeForm";
import { FaceBody, NodeShell } from "./NodeShell";
import { ProcessorFooter } from "./ProcessorFooter";
import { ProcessorError } from "./ProcessorHealth";
import { ageLabel, processorGate, processorLanes } from "./processorFace";
import { RadarPlot } from "./RadarPlot";
import {
  bearingCell,
  isStale,
  RADAR_TX_PORT,
  radarChips,
  radarSubtitle,
  sortTracks,
  TRACK_ROWS,
  truthCell,
} from "./radar";

const AGE_TICK_MS = 1_000;
const CLEAR_ACTION = "Clear";
const TRACK_HEADS = ["Track", "km", "m/s", "Bearing", "dB", "Truth"] as const;

function useTrackClear(node: string): { clear: () => void; pending: boolean } {
  const mutation = useMutation({
    mutationFn: () => clearRadarTracks(node),
    onSuccess: () => clearAction(node, CLEAR_ACTION),
    onError: (error) => failAction(node, CLEAR_ACTION, error),
  });
  return { clear: () => mutation.mutate(), pending: mutation.isPending };
}

export function RadarFace({ node }: { node: PatchNode }) {
  const workspace = useWorkspaceContext();
  const state = useProcessorStore((store) => store.byNode[node.id]);
  const array = arrayOf(workspace.resolved, node.id);
  const status = useArrayStore((store) => (array === null ? undefined : store.byNode[array]));
  const now = useNow(AGE_TICK_MS);
  const [colormap, setColormap] = useState<Colormap>(DEFAULT_COLORMAP);
  const [selected, setSelected] = useState<number | null>(null);
  if (node.kind !== "passive_radar") {
    return null;
  }
  const settings = nodeSettings(workspace.context.catalog, node) as PassiveRadarParams | null;
  const update = readingOf(state, "passive_radar");
  const stale = state !== undefined && update !== null && isStale(state.receivedAt, update, now);
  const lanes = processorLanes(workspace.resolved, array, status);
  const health = processorStatusOf(status, node.id);
  const subtitle = radarSubtitle({
    wired: array !== null,
    txWired: hasFeed(workspace.resolved, node.id, RADAR_TX_PORT),
    gate: processorGate(status, node.id),
    update,
    stale,
    illuminator: settings?.illuminator.kind ?? "fm",
    lanes,
  });
  return (
    <NodeShell
      node={node}
      title="Passive radar"
      category="tool"
      subtitle={<span className={subtitle.warn ? "text-warn" : undefined}>{subtitle.text}</span>}
    >
      <FaceBody scroll={false}>
        <div className="flex min-h-0 flex-1 flex-col">
          <RadarPlot
            node={node.id}
            known={health !== null}
            update={update}
            dim={stale || update === null}
            colormap={colormap}
            selected={selected}
          />
          <StatusGrid
            readouts={readoutsOf(state)}
            age={ageLabel(state?.receivedAt, now)}
            columns={4}
          />
          <TrackTable update={update} selected={selected} onSelect={setSelected} />
          <ProcessorError status={health} />
          <div className="max-h-56 shrink-0 overflow-y-auto">
            <NodeForm
              node={node}
              lanes={lanes}
              lead={
                <ChoiceChip
                  label="Colours"
                  value={colormap}
                  options={COLOUR_OPTIONS}
                  title="Plot colours"
                  onChange={setColormap}
                />
              }
            />
          </div>
        </div>
      </FaceBody>
      <ProcessorFooter
        status={health}
        chips={radarChips(update)}
        actions={<ClearTracks node={node.id} cleared={update === null} />}
      />
    </NodeShell>
  );
}

function ClearTracks({ node, cleared }: { node: string; cleared: boolean }) {
  const { clear, pending } = useTrackClear(node);
  return (
    <Button
      type="button"
      className={BTN}
      title="Forget every track"
      disabled={pending || cleared}
      onClick={clear}
    >
      Clear
    </Button>
  );
}

function TrackTable({
  update,
  selected,
  onSelect,
}: {
  update: RadarUpdate | null;
  selected: number | null;
  onSelect: (id: number | null) => void;
}) {
  const tracks = sortTracks(update?.tracks ?? [], TRACK_ROWS);
  if (tracks.length === 0) {
    return null;
  }
  return (
    <table className="w-full shrink-0 border-t border-line">
      <thead>
        <tr>
          {TRACK_HEADS.map((head) => (
            <th key={head} className={TABLE_HEAD}>
              {head}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {tracks.map((track) => {
          const bearing = bearingCell(track.aoa);
          const chosen = track.id === selected;
          return (
            <tr
              key={track.id}
              className={`${chosen ? "bg-accent/15" : "hover:bg-panel-2"} ${
                track.state === "coasting" ? "opacity-45" : ""
              }`}
            >
              <td className={TABLE_CELL}>
                <Button
                  type="button"
                  aria-pressed={chosen}
                  title={track.state === "coasting" ? "Coasting" : "Show on the plot"}
                  className="cursor-pointer font-mono hover:text-accent"
                  onClick={() => onSelect(chosen ? null : track.id)}
                >
                  T{track.id}
                </Button>
              </td>
              <td className={TABLE_CELL}>{track.range_km.toFixed(1)}</td>
              <td className={TABLE_CELL}>{track.range_rate_mps.toFixed(0)}</td>
              <td
                className={`${TABLE_CELL} ${bearing.arrayFrame ? "text-ink-faint" : ""}`}
                title={bearing.arrayFrame ? "Array frame" : undefined}
              >
                {bearing.text}
              </td>
              <td className={TABLE_CELL}>{track.snr_db.toFixed(0)}</td>
              <td className={`${TABLE_CELL} max-w-20 truncate`}>{truthCell(track)}</td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}
