import { useMutation } from "@tanstack/react-query";
import { Button } from "../../components/BaseControls";
import { FACE_BTN, FACE_BTN_DANGER } from "../../components/controls";
import { Chips, ChoiceChip } from "../../components/face/Chips";
import { FaceFault } from "../../components/face/Fault";
import { Readout, Readouts } from "../../components/face/Readouts";
import { FaceStats, Stat } from "../../components/face/Stats";
import { TextChip } from "../../components/face/TextChip";
import {
  DROPS_HINT,
  formatBytes,
  formatCount,
  formatHz,
  formatSampleRate,
} from "../../components/format";
import {
  channelExportSource,
  deriveNetworkExportControl,
  deviceExportSource,
  type NetworkExportSource,
  networkExportControlsLocked,
  networkExportMutationOptions,
  nodeExportSource,
} from "../../components/networkExport";
import { ownSourceOf } from "../../lib/resolved";
import type { NetworkExportStatus, PatchNode, PatchNodeOf } from "../../lib/types";
import { useWorkspaceNow, useWorkspaceSelect, type Workspace } from "../context";
import { patchNode } from "../graph";
import { Inspect, InspectSection } from "../inspector";
import { basebandSourceOf } from "../lanes";
import { runningSetOf } from "../workspaceDevice";
import { FaceBody, FaceFooter, NodeShell } from "./NodeShell";

const TRANSPORTS = [
  { value: "udp", label: "UDP datagrams" },
  { value: "tcp", label: "TCP stream" },
  { value: "rtl_tcp", label: "rtl_tcp server (rtl_433)" },
] as const;

const FORMATS = [
  { value: "cf32_le", label: "Complex float 32 LE" },
  { value: "ci16_le", label: "Complex int 16 LE" },
  { value: "cu8", label: "Complex unsigned 8" },
] as const;

export function NetworkExportFace({ node }: { node: PatchNode }) {
  if (node.kind !== "network_export") {
    return null;
  }
  return <NetworkExportNodeFace node={node} />;
}

function NetworkExportNodeFace({ node }: { node: PatchNodeOf<"network_export"> }) {
  const now = useWorkspaceNow();
  const { source, baseband } = useWorkspaceSelect((workspace) => exportView(workspace, node.id));
  const control = deriveNetworkExportControl(source, node.id);
  const settings = {
    transport: node.data.transport,
    format: node.data.format,
    address: node.data.address,
  };
  const edit = (next: Partial<typeof settings>) => {
    now().edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (current) =>
        current.kind === "network_export"
          ? { ...current, data: { ...current.data, ...next } }
          : current,
      ),
    }));
  };
  const exportIq = useMutation(networkExportMutationOptions(node.id));
  const locked = networkExportControlsLocked(control, exportIq.isPending);
  const rtlTcp = node.data.transport === "rtl_tcp";
  const active = control.kind === "active" ? control.status : null;

  return (
    <NodeShell node={node} title={baseband ? "Network baseband" : "Network IQ"} category="output">
      <Inspect node={node.id}>
        <InspectSection>
          <Chips inset="none">
            <ChoiceChip
              label="Transport"
              title="Transport"
              value={node.data.transport}
              options={TRANSPORTS}
              disabled={locked}
              onChange={(transport) =>
                edit(
                  transport === "rtl_tcp"
                    ? { transport, format: "cu8", address: "127.0.0.1:1234" }
                    : { transport },
                )
              }
            />
            <ChoiceChip
              label="Samples"
              title="Sample format"
              value={node.data.format}
              options={FORMATS}
              disabled={locked || rtlTcp}
              onChange={(format) => edit({ format })}
            />
            <TextChip
              label={rtlTcp ? "Listen" : "To"}
              name={rtlTcp ? "rtl_tcp listen address" : "Network IQ destination"}
              title={
                rtlTcp
                  ? "Exports the wired source. Set rtl_433 frequency and sample rate to match; client tuning commands are ignored."
                  : "Network IQ destination"
              }
              value={node.data.address}
              disabled={locked}
              onCommit={(address) => {
                if (address !== "") {
                  edit({ address });
                }
              }}
            />
          </Chips>
        </InspectSection>
      </Inspect>
      <FaceBody>
        <Readouts columns={2}>
          <Readout label="Rate">
            {active === null ? null : formatSampleRate(active.sample_rate)}
          </Readout>
          <Readout label="Center">{active === null ? null : formatHz(active.center_hz)}</Readout>
        </Readouts>
        {active?.error != null && <FaceFault message={active.error} />}
        {control.kind === "busy" && (
          <FaceFault message="Another network sink already uses this input" />
        )}
      </FaceBody>
      <FaceFooter>
        <ExportStats status={active} transport={node.data.transport} />
        {control.kind === "active" ? (
          <Button
            type="button"
            className={FACE_BTN_DANGER}
            disabled={exportIq.isPending}
            onClick={() => exportIq.mutate("stop")}
          >
            Stop
          </Button>
        ) : (
          <Button
            type="button"
            className={FACE_BTN}
            title={rtlTcp ? "rtl_433 input, CU8" : "Raw interleaved I/Q"}
            disabled={control.kind !== "ready" || exportIq.isPending}
            onClick={() => exportIq.mutate("start")}
          >
            Start export
          </Button>
        )}
      </FaceFooter>
    </NodeShell>
  );
}

function exportView(
  workspace: Workspace,
  id: string,
): { source: NetworkExportSource | null; baseband: boolean } {
  const set = runningSetOf(workspace, id);
  const channel = basebandSourceOf(workspace.resolved, workspace.deviceSets, id);
  const encoder = ownSourceOf(workspace.resolved, id, "baseband");
  if (encoder !== null) {
    const stream = workspace.nodeStreams.find((made) => made.node === encoder) ?? null;
    return { source: nodeExportSource(stream), baseband: true };
  }
  if (channel === null) {
    return { source: deviceExportSource(set), baseband: false };
  }
  const owner =
    [...workspace.devices.values()].find((bound) => bound.id === channel.deviceSet) ?? null;
  return { source: channelExportSource(owner, channel.channel), baseband: true };
}

/** What the export has sent; zero before it starts, so the footer keeps its shape. */
function ExportStats({
  status,
  transport,
}: {
  status: NetworkExportStatus | null;
  transport: NetworkExportStatus["settings"]["transport"];
}) {
  const sending = status?.settings.transport ?? transport;
  const overruns = status?.overruns ?? 0;
  return (
    <FaceStats>
      {sending === "rtl_tcp" && (
        <Stat label="Clients" title="rtl_tcp clients connected">
          {status?.clients ?? 0}
        </Stat>
      )}
      <Stat label="Sent" title="Bytes sent">
        {formatBytes(status?.bytes ?? 0)}
      </Stat>
      <Stat
        label={sending === "udp" ? "Datagrams" : "Writes"}
        title={sending === "udp" ? "Datagrams sent" : "Socket writes"}
      >
        {formatCount(status?.packets ?? 0)}
      </Stat>
      {overruns > 0 && (
        <Stat label="Drops" title={DROPS_HINT} tone="warn">
          {formatCount(overruns)}
        </Stat>
      )}
    </FaceStats>
  );
}
