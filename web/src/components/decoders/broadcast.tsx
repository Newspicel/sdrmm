import { EMPTY, PANE, type PanelProps } from "./panelKit";

export function BroadcastView({ records }: PanelProps<"broadcast">) {
  const status = records[0]?.event.data;
  return (
    <div className={PANE}>
      <span className="legend">{status?.label ?? "Broadcast service"}</span>
      {status?.dynamic_label && <p className="text-sm text-ink">{status.dynamic_label}</p>}
      {!status && <span className={EMPTY}>Waiting for a broadcast service.</span>}
    </div>
  );
}
