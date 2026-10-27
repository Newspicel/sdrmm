import { BroadcastDataView, EMPTY, PANE, type PanelProps } from "./panelKit";

export function BroadcastDataPanel({ records }: PanelProps<"broadcast_data">) {
  const object = records[0]?.event.data;
  return (
    <div className={PANE}>
      {object ? (
        <BroadcastDataView data={object} />
      ) : (
        <span className={EMPTY}>No data object yet.</span>
      )}
    </div>
  );
}
