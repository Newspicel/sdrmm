import { useMutation } from "@tanstack/react-query";
import { Button } from "../../components/BaseControls";
import { BTN_DANGER, BTN_PRIMARY } from "../../components/controls";
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
  networkExportControlsLocked,
  networkExportMutationOptions,
} from "../../components/networkExport";
import type { NetworkExportStatus, PatchNode, PatchNodeOf } from "../../lib/types";
import { useWorkspaceContext } from "../context";
import { patchNode } from "../graph";
import { basebandSourceOf, hasFeed } from "../lanes";
import { deviceSetOf } from "../workspaceDevice";
import { FaceBody, FaceEmpty, FaceFooter, NodeShell } from "./NodeShell";

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
  const workspace = useWorkspaceContext();
  const set = deviceSetOf(workspace, node.id);
  const channel = basebandSourceOf(workspace.resolved, workspace.deviceSets, node.id);
  const owner =
    channel === null
      ? set
      : ([...workspace.devices.values()].find((bound) => bound.id === channel.deviceSet) ?? null);
  const wired =
    hasFeed(workspace.resolved, node.id, "iq") || hasFeed(workspace.resolved, node.id, "baseband");
  const control = deriveNetworkExportControl(
    channel === null ? deviceExportSource(set) : channelExportSource(owner, channel.channel),
    node.id,
  );
  const settings = {
    transport: node.data.transport,
    format: node.data.format,
    address: node.data.address,
  };
  const edit = (next: Partial<typeof settings>) => {
    workspace.edit((snapshot) => ({
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

  return (
    <NodeShell
      node={node}
      title={channel === null ? "Network IQ" : "Network baseband"}
      category="output"
    >
      <FaceBody>
        <Chips>
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
        {!wired ? (
          <FaceEmpty hint="Wire a device's IQ or a channel's baseband in" />
        ) : control.kind === "active" ? (
          <>
            <Readouts columns={2}>
              <Readout label="Rate">{formatSampleRate(control.status.sample_rate)}</Readout>
              <Readout label="Center">{formatHz(control.status.center_hz)}</Readout>
            </Readouts>
            {control.status.error != null && <FaceFault message={control.status.error} />}
          </>
        ) : (
          <FaceEmpty
            hint={
              control.kind === "busy" ? "Another network sink already uses this input" : undefined
            }
          />
        )}
      </FaceBody>
      <FaceFooter>
        {control.kind === "active" && <ExportStats status={control.status} rtlTcp={rtlTcp} />}
        {control.kind === "active" ? (
          <Button
            type="button"
            className={BTN_DANGER}
            disabled={exportIq.isPending}
            onClick={() => exportIq.mutate("stop")}
          >
            Stop
          </Button>
        ) : (
          <Button
            type="button"
            className={BTN_PRIMARY}
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

function ExportStats({ status, rtlTcp }: { status: NetworkExportStatus; rtlTcp: boolean }) {
  return (
    <FaceStats>
      {rtlTcp && (
        <Stat label="Clients" title="rtl_tcp clients connected">
          {status.clients ?? 0}
        </Stat>
      )}
      <Stat label="Sent" title="Bytes sent">
        {formatBytes(status.bytes)}
      </Stat>
      <Stat
        label={status.settings.transport === "udp" ? "Datagrams" : "Writes"}
        title={status.settings.transport === "udp" ? "Datagrams sent" : "Socket writes"}
      >
        {formatCount(status.packets)}
      </Stat>
      {status.overruns > 0 && (
        <Stat label="Drops" title={DROPS_HINT} tone="warn">
          {formatCount(status.overruns)}
        </Stat>
      )}
    </FaceStats>
  );
}
