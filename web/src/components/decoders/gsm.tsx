import { type Mode, ToggleChip } from "./kit";

export function GsmChips({ params, onParams }: Mode<"gsm">) {
  const settings = params.settings;
  return (
    <>
      <ToggleChip
        label="Paging"
        title="Show paging and assignments"
        on={settings.paging}
        onChange={(paging) => onParams({ type: "gsm", settings: { ...settings, paging } })}
      />
      <ToggleChip
        label="Follow"
        title="Follow assigned SDCCH on this carrier"
        on={settings.follow}
        onChange={(follow) => onParams({ type: "gsm", settings: { ...settings, follow } })}
      />
    </>
  );
}

import {
  EMPTY,
  formatClock,
  type GsmCellView,
  gsmCells,
  gsmSummary,
  PANE,
  type PanelProps,
  Readout,
  Readouts,
  TABLE_CELL,
  TABLE_HEAD,
} from "./panelKit";

const GSM_LOG_ROWS = 50;

function GsmCellReadouts({ cell }: { cell: GsmCellView }) {
  return (
    <Readouts ruled={false} padded={false} columns={2} label={cell.cgi ?? "Cell"}>
      <Readout label="ARFCN" title={cell.band ?? undefined}>
        {cell.arfcn ?? "-"}
      </Readout>
      <Readout label="BSIC" title="Network and base station colour code">
        {cell.bsic}
      </Readout>
      <Readout label="CCCH">{cell.ccch ?? "-"}</Readout>
      <Readout label="Neighbours" title={cell.neighbours.join(", ")}>
        {cell.neighbours.length}
      </Readout>
      <Readout label="SI" title={cell.systemInformation.join(", ")}>
        {cell.systemInformation.length}
      </Readout>
      <Readout label="Paged">{cell.paged}</Readout>
      <Readout label="Offset">{Math.round(cell.frequencyErrorHz)} Hz</Readout>
      <Readout label="SNR">{cell.snrDb.toFixed(1)} dB</Readout>
    </Readouts>
  );
}

export function GsmView({ records }: PanelProps<"gsm">) {
  if (records.length === 0) {
    return (
      <div className={PANE}>
        <span className={EMPTY}>No GSM cell heard yet.</span>
      </div>
    );
  }
  const log = records.slice(0, GSM_LOG_ROWS);
  return (
    <div className={PANE}>
      {gsmCells(records).map((cell) => (
        <GsmCellReadouts key={cell.key} cell={cell} />
      ))}
      <div className="overflow-x-auto">
        <table className="w-full border-collapse text-left text-xs">
          <thead>
            <tr className="border-b border-line">
              <th className={TABLE_HEAD}>Time</th>
              <th className={TABLE_HEAD}>FN</th>
              <th className={TABLE_HEAD}>TS</th>
              <th className={TABLE_HEAD}>Message</th>
            </tr>
          </thead>
          <tbody>
            {log.map((record) => {
              const frame = record.event.data;
              return (
                <tr
                  key={`${record.at}:${frame.frame_number}:${frame.timeslot}`}
                  className="border-b border-line/40"
                >
                  <td className={`${TABLE_CELL} tabular-nums`}>{formatClock(record.at)}</td>
                  <td className={`${TABLE_CELL} tabular-nums`}>{frame.frame_number}</td>
                  <td className={`${TABLE_CELL} tabular-nums`}>{frame.timeslot}</td>
                  <td className={`${TABLE_CELL} max-w-96 truncate`} title={frame.raw}>
                    {gsmSummary(frame)}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}
