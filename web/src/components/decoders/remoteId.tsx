import { ChoiceChip, choicesOf, type Mode } from "./kit";

export function RemoteIdChips({ params, choices, onParams }: Mode<"remote_id">) {
  return (
    <ChoiceChip
      label="Link"
      title="Radio link to listen on"
      value={params.settings.link}
      options={choicesOf(choices, params.settings, "link")}
      onChange={(link) => onParams({ type: "remote_id", settings: { ...params.settings, link } })}
    />
  );
}

import {
  ageClass,
  EMPTY,
  formatAge,
  PANE,
  type PanelProps,
  TABLE_CELL,
  TABLE_HEAD,
  Unit,
  useNow,
} from "./panelKit";

function wholeMetres(value: number | null): string {
  return value === null ? "-" : value.toFixed(0);
}

function DroneRow({ drone, now }: { drone: Drone; now: number }) {
  const ageMs = now - Date.parse(drone.at);
  return (
    <tr className={`border-b border-line/40 ${ageClass(ageMs)}`}>
      <td className={`${TABLE_CELL} font-mono`}>{drone.key}</td>
      <td className={TABLE_CELL}>{drone.kind}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>{wholeMetres(drone.heightM)}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>
        {drone.speedMps === null ? "-" : drone.speedMps.toFixed(1)}
      </td>
      <td className={`${TABLE_CELL} tabular-nums`}>{wholeMetres(drone.pilotM)}</td>
      <td className={`${TABLE_CELL} max-w-40 truncate`} title={drone.operator ?? undefined}>
        {drone.operator ?? "-"}
      </td>
      <td className={TABLE_CELL}>{drone.links.join(", ")}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>{drone.levelDbfs.toFixed(0)}</td>
      <td className={`${TABLE_CELL} text-right`}>{formatAge(ageMs)}</td>
    </tr>
  );
}

export function RemoteIdView({ records }: PanelProps<"remote_id">) {
  const now = useNow(1000);
  const drones = remoteIdDrones(records);
  if (drones.length === 0) {
    return (
      <div className={PANE}>
        <span className={EMPTY}>No drones heard yet.</span>
      </div>
    );
  }
  return (
    <div className={PANE}>
      <div className="overflow-x-auto">
        <table className="w-full border-collapse text-left text-xs">
          <thead>
            <tr className="border-b border-line">
              <th className={TABLE_HEAD}>Drone</th>
              <th className={TABLE_HEAD}>Type</th>
              <th className={TABLE_HEAD}>
                Height <Unit symbol="m" />
              </th>
              <th className={TABLE_HEAD}>
                Speed <Unit symbol="m/s" />
              </th>
              <th className={TABLE_HEAD} title="Distance from drone to pilot">
                Pilot <Unit symbol="m" />
              </th>
              <th className={TABLE_HEAD}>Operator</th>
              <th className={TABLE_HEAD}>Link</th>
              <th className={TABLE_HEAD}>
                Level <Unit symbol="dBFS" />
              </th>
              <th className={TABLE_HEAD}>Heard</th>
            </tr>
          </thead>
          <tbody>
            {drones.map((drone) => (
              <DroneRow key={drone.key} drone={drone} now={now} />
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

import {
  distanceM,
  REMOTE_ID_TRANSPORT_LABELS,
  remoteIdLocation,
  remoteIdOperator,
  remoteIdPilot,
  remoteIdPosition,
  remoteIdStation,
  remoteIdUaType,
  UA_TYPE_LABELS,
  withRemoteIdState,
} from "../../lib/remoteId";
import { type DecodedRecordOf } from "../../lib/types";
import { chronological } from "../decoderViews";

export interface Drone {
  key: string;
  kind: string;
  heightM: number | null;
  speedMps: number | null;
  pilotM: number | null;
  operator: string | null;
  links: string[];
  levelDbfs: number;
  frames: number;
  at: string;
}

export function remoteIdDrones(records: readonly DecodedRecordOf<"remote_id">[]): Drone[] {
  const states = new Map<
    string,
    { frame: DecodedRecordOf<"remote_id">["event"]["data"]; drone: Drone }
  >();
  for (const record of chronological(records)) {
    const next = record.event.data;
    const key = remoteIdStation(next);
    const previous = states.get(key);
    const frame = previous ? withRemoteIdState(previous.frame, next) : next;
    const location = remoteIdLocation(frame);
    const drone = remoteIdPosition(frame);
    const pilot = remoteIdPilot(frame);
    const uaType = remoteIdUaType(frame);
    const link = REMOTE_ID_TRANSPORT_LABELS[next.transport];
    const links = previous?.drone.links ?? [];
    states.set(key, {
      frame,
      drone: {
        key,
        kind: uaType === null ? "-" : UA_TYPE_LABELS[uaType],
        heightM: location?.height_m ?? null,
        speedMps: location?.speed_mps ?? null,
        pilotM: drone !== null && pilot !== null ? distanceM(drone, pilot) : null,
        operator: remoteIdOperator(frame),
        links: links.includes(link) ? links : [...links, link],
        levelDbfs: next.level_dbfs,
        frames: (previous?.drone.frames ?? 0) + 1,
        at: record.at,
      },
    });
  }
  return [...states.values()]
    .map((state) => state.drone)
    .toSorted((a, b) => Date.parse(b.at) - Date.parse(a.at));
}
