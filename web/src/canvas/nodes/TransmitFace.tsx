import { useMutation } from "@tanstack/react-query";
import { Button } from "../../components/BaseControls";
import { FACE_BTN_DANGER, FACE_BTN_WARN } from "../../components/controls";
import {
  formatPeak,
  METER_FLOOR_DB,
  type MeterTone,
  meterTone,
  meterUnit,
} from "../../components/dbfs";
import { ANY_FREQUENCY, inTuningRange, tuningRange } from "../../components/dial";
import { dialId, FrequencyDial } from "../../components/FrequencyDial";
import { Chips, NumberChip } from "../../components/face/Chips";
import { FaceFault } from "../../components/face/Fault";
import { MeterBar, MeterRow } from "../../components/face/Meter";
import { FaceStats, Stat } from "../../components/face/Stats";
import { formatCount, formatHz, formatMhz } from "../../components/format";
import { HzChip } from "../../components/HzField";
import { TuneTo } from "../../components/TuneTo";
import { nodeAction } from "../../lib/api";
import { reachesOf, useFeeds } from "../../lib/resolved";
import { toastError } from "../../lib/toasts";
import type { PatchNode, PatchNodeOf, TransmitAction, TransmitSettings } from "../../lib/types";
import { useWorkspaceNow, useWorkspaceSelect, type Workspace } from "../context";
import { nodeOf, patchNode } from "../graph";
import { Inspect, InspectSection } from "../inspector";
import { runningSetOf } from "../workspaceDevice";
import { AirLabel } from "./AirLabel";
import { airOf } from "./airState";
import { FaceBody, FaceFooter, NodeShell } from "./NodeShell";
import { radioUseOf } from "./radioUse";
import {
  MAX_TIMEOUT_S,
  MIN_LEVEL_DB,
  tooWide,
  transmitStatusOf,
  withTransmitSettings,
} from "./transmit";

export function TransmitFace({ node }: { node: PatchNode }) {
  if (node.kind !== "transmit") {
    return null;
  }
  return <TransmitNodeFace node={node} />;
}

function TransmitNodeFace({ node }: { node: PatchNodeOf<"transmit"> }) {
  const now = useWorkspaceNow();
  const { status, wired, receiveOnly, range, centerHz, selected } = useWorkspaceSelect(
    (workspace) => transmitView(workspace, node.id),
  );
  const air = airOf(status);
  const fed = useFeeds(node.id, "iq").length + useFeeds(node.id, "baseband").length > 0;
  const need = transmitNeed(fed, wired, receiveOnly);
  const frequency = node.data.frequency_hz ?? status?.frequency_hz ?? centerHz;
  const tune = (frequency_hz: number) => edit({ frequency_hz });
  const edit = (next: Partial<TransmitSettings>): void =>
    now().edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (current) => withTransmitSettings(current, next)),
    }));
  const act = useMutation({
    mutationFn: (action: TransmitAction) =>
      nodeAction(node.id, { action: "transmit", with: action }),
    onError: (error: Error) => toastError(error),
  });

  return (
    <NodeShell
      node={node}
      title="Transmitter"
      category="output"
      subtitle={air === null ? undefined : <AirLabel air={air} />}
    >
      <FaceBody>
        <div className="@container flex flex-col gap-1 p-2">
          <div className="flex min-w-0 items-center gap-1">
            <FrequencyDial
              id={dialId(node.id)}
              hz={frequency ?? 0}
              range={range}
              wheelTunes={selected}
              onTune={tune}
            />
            <span className="ml-auto flex shrink-0 items-center gap-1">
              <TuneTo
                title="Type the frequency to send on"
                hz={frequency ?? 0}
                hint={`Reaches ${formatMhz(range.min)} to ${formatMhz(range.max)}`}
                resolve={(entered) => inTuningRange(entered, range)}
                onTune={tune}
              />
            </span>
          </div>
          {need === null ? (
            <DriveRow peakDb={status?.peak_db ?? undefined} />
          ) : (
            <NeedRow need={need} />
          )}
        </div>
        {status !== null && tooWide(status) && status.occupied_hz != null && (
          <FaceFault
            message={`${formatHz(status.occupied_hz)} wide, over its ${formatHz(status.channel_hz ?? 0)} channel`}
          />
        )}
        {status?.error != null && <FaceFault message={status.error} />}
      </FaceBody>
      <Inspect node={node.id}>
        <InspectSection>
          <Chips inset="none">
            <HzChip
              label="Freq"
              title="Transmit frequency. Empty: an encoder sits on the radio's centre, a lane where it was received"
              value={node.data.frequency_hz ?? null}
              placeholder="auto"
              optional
              min={1}
              onCommit={(frequency_hz) => edit({ frequency_hz })}
            />
            <NumberChip
              label="Level"
              title="Digital drive below full scale"
              value={node.data.level_db}
              unit="dB"
              min={MIN_LEVEL_DB}
              max={0}
              step={1}
              onCommit={(level_db) => edit({ level_db })}
            />
            <NumberChip
              label="Timeout"
              title="Longest single transmission before it disarms itself"
              value={node.data.timeout_s}
              unit="s"
              min={1}
              max={MAX_TIMEOUT_S}
              step={1}
              onCommit={(timeout_s) => edit({ timeout_s })}
            />
          </Chips>
        </InspectSection>
      </Inspect>
      <FaceFooter>
        {status !== null && (
          <FaceStats>
            <Stat label="Sent" title="Transmissions since arming">
              {formatCount(status.bursts)}
            </Stat>
            {status.underruns > 0 && (
              <Stat label="Drops" title="Times the source ran dry or overran" tone="warn">
                {formatCount(status.underruns)}
              </Stat>
            )}
          </FaceStats>
        )}
        {status === null ? (
          <Button
            type="button"
            className={FACE_BTN_WARN}
            title={
              receiveOnly ? SET_TO_SEND : "From here on, what the source produces goes on the air"
            }
            disabled={act.isPending || receiveOnly}
            onClick={() => act.mutate({ do: "arm" })}
          >
            Arm
          </Button>
        ) : (
          <Button
            type="button"
            className={FACE_BTN_DANGER}
            disabled={act.isPending}
            onClick={() => act.mutate({ do: "disarm" })}
          >
            Disarm
          </Button>
        )}
      </FaceFooter>
    </NodeShell>
  );
}

/** What the face shows of the transmitter and the radio its wire reaches. */
function transmitView(workspace: Workspace, node: string) {
  const radio = reachesOf(workspace.resolved, node, "transmit")[0]?.sink ?? null;
  const set = radio === null ? null : runningSetOf(workspace, radio);
  return {
    status: transmitStatusOf(workspace.deviceSets, node),
    wired: radio !== null,
    receiveOnly: radio !== null && radioUseOf(nodeOf(workspace.graph, radio), set) === "receive",
    range: set === null ? ANY_FREQUENCY : tuningRange(set.capabilities),
    centerHz: set?.settings.center_hz ?? null,
    selected: workspace.selected === node,
  };
}

const SET_TO_SEND = "Set the radio to send";

/** What the transmitter says it needs before Arm can key it. A missing wire it leaves unsaid. */
export function transmitNeed(fed: boolean, wired: boolean, receiveOnly: boolean): string | null {
  return fed && wired && receiveOnly ? SET_TO_SEND : null;
}

const TONE: Record<MeterTone, string> = {
  ok: "bg-ok",
  warn: "bg-warn",
  danger: "bg-danger",
};

/** Peak drive of the latest transmission, at the floor while nothing goes out. */
function DriveRow({ peakDb }: { peakDb: number | undefined }) {
  const quiet = formatPeak(peakDb) === "quiet";
  return (
    <MeterRow
      label="Drive"
      title="Peak drive of the latest transmission, below full scale"
      meter={
        <MeterBar
          label="Drive"
          value={meterUnit(peakDb)}
          valueText={formatPeak(peakDb)}
          fill={TONE[meterTone(peakDb, false)]}
        />
      }
      readout={
        <span className={quiet ? "text-ink-faint" : ""}>
          {quiet ? `${METER_FLOOR_DB} dB` : `${(peakDb ?? 0).toFixed(1)} dB`}
        </span>
      }
      solo
    />
  );
}

/** What the transmitter still needs, in the row its drive meter takes once it has it. */
function NeedRow({ need }: { need: string }) {
  return <div className="flex h-7 items-center justify-center text-xs text-ink-faint">{need}</div>;
}
