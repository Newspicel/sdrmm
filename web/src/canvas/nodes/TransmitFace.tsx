import { useMutation } from "@tanstack/react-query";
import { Button } from "../../components/BaseControls";
import { BTN_DANGER_ARMED, BTN_PRIMARY } from "../../components/controls";
import { Chips, NumberChip } from "../../components/face/Chips";
import { FaceFault } from "../../components/face/Fault";
import { Readout, Readouts } from "../../components/face/Readouts";
import { FaceStats, Stat } from "../../components/face/Stats";
import { formatCount, formatHz } from "../../components/format";
import { HzChip } from "../../components/HzField";
import { nodeAction } from "../../lib/api";
import { toastError } from "../../lib/toasts";
import type { PatchNode, PatchNodeOf, TransmitAction, TransmitSettings } from "../../lib/types";
import { useWorkspaceContext } from "../context";
import { patchNode } from "../graph";
import { FaceBody, FaceFooter, NodeShell } from "./NodeShell";
import {
  MAX_TIMEOUT_S,
  MIN_LEVEL_DB,
  peakText,
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
  const workspace = useWorkspaceContext();
  const status = transmitStatusOf(workspace.deviceSets, node.id);
  const onAir = status?.phase === "on_air";
  const edit = (next: Partial<TransmitSettings>): void =>
    workspace.edit((snapshot) => ({
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
      subtitle={status === null ? undefined : "armed"}
      badge={
        onAir ? (
          <span role="status" className="font-semibold text-danger" title="Transmitting now">
            ON AIR
          </span>
        ) : undefined
      }
    >
      <FaceBody>
        <Chips>
          <HzChip
            label="Freq"
            title="Transmit frequency. Empty: a modulator sits on the radio's centre, a lane where it was received"
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
        {status !== null && (
          <>
            <Readouts columns={2}>
              <Readout label="On">{formatHz(status.frequency_hz)}</Readout>
              <Readout label="Peak" title="Peak drive of the latest transmission">
                {peakText(status)}
              </Readout>
              {status.occupied_hz != null && (
                <Readout
                  label="Width"
                  tone={tooWide(status) ? "warn" : undefined}
                  title={
                    tooWide(status) && status.channel_hz != null
                      ? `${formatHz(status.occupied_hz)} is wider than its ${formatHz(status.channel_hz)} channel`
                      : "Band holding 99 % of the latest transmission's power"
                  }
                >
                  {formatHz(status.occupied_hz)}
                </Readout>
              )}
            </Readouts>
            {status.error != null && <FaceFault message={status.error} />}
          </>
        )}
      </FaceBody>
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
            className={BTN_PRIMARY}
            title="From here on, what the source produces goes on the air"
            disabled={act.isPending}
            onClick={() => act.mutate({ do: "arm" })}
          >
            Arm
          </Button>
        ) : (
          <Button
            type="button"
            className={BTN_DANGER_ARMED}
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
