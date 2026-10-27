import { ChoiceChip, choicesOf, type Mode } from "./kit";

export function BleChips({ params, choices, onParams }: Mode<"ble">) {
  return (
    <ChoiceChip
      label="Link"
      title="Bluetooth channels to listen on"
      value={params.settings.link}
      options={choicesOf(choices, params.settings, "link")}
      onChange={(link) => onParams({ type: "ble", settings: { ...params.settings, link } })}
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

function BleRow({ device, now }: { device: BleDevice; now: number }) {
  const ageMs = now - Date.parse(device.at);
  return (
    <tr className={`border-b border-line/40 ${ageClass(ageMs)}`}>
      <td className={`${TABLE_CELL} font-mono`}>{device.key}</td>
      <td className={`${TABLE_CELL} max-w-40 truncate`} title={device.name ?? undefined}>
        {device.name ?? "-"}
      </td>
      <td className={TABLE_CELL}>{device.vendor ?? "-"}</td>
      <td className={TABLE_CELL}>{device.beacon ?? "-"}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>{device.channel ?? "-"}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>{device.levelDbfs.toFixed(0)}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>{device.heard}</td>
      <td className={`${TABLE_CELL} text-right`}>{formatAge(ageMs)}</td>
    </tr>
  );
}

export function BleView({ records }: PanelProps<"ble">) {
  const now = useNow(1000);
  const devices = bleDevices(records);
  if (devices.length === 0) {
    return (
      <div className={PANE}>
        <span className={EMPTY}>No Bluetooth devices heard yet.</span>
      </div>
    );
  }
  return (
    <div className={PANE}>
      <div className="overflow-x-auto">
        <table className="w-full border-collapse text-left text-xs">
          <thead>
            <tr className="border-b border-line">
              <th className={TABLE_HEAD}>Address</th>
              <th className={TABLE_HEAD}>Name</th>
              <th className={TABLE_HEAD}>Vendor</th>
              <th className={TABLE_HEAD}>Beacon</th>
              <th className={TABLE_HEAD}>Ch</th>
              <th className={TABLE_HEAD}>
                Level <Unit symbol="dBFS" />
              </th>
              <th className={TABLE_HEAD} title="Adverts heard, repeats included">
                Adverts
              </th>
              <th className={TABLE_HEAD}>Heard</th>
            </tr>
          </thead>
          <tbody>
            {devices.map((device) => (
              <BleRow key={device.key} device={device} now={now} />
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

import { BLE_BEACON_LABELS, BLE_PDU_LABELS, bleStation, bleVendor } from "../../lib/ble";
import { type BleAdvert, type DecodedRecordOf } from "../../lib/types";
import { chronological } from "../decoderViews";

export interface BleDevice {
  key: string;
  name: string | null;
  vendor: string | null;
  beacon: string | null;
  kind: string;
  channel: number | null;
  levelDbfs: number;
  heard: number;
  at: string;
}

export function bleAnonymousKey(advert: BleAdvert): string {
  const label = BLE_PDU_LABELS[advert.pdu];
  return advert.adi == null
    ? `${label} ${advert.data.slice(0, 12)}`
    : `${label} SID ${advert.adi.set} DID ${advert.adi.data_id.toString(16).toUpperCase()}`;
}

export function bleDevices(records: readonly DecodedRecordOf<"ble">[]): BleDevice[] {
  const devices = new Map<string, BleDevice>();
  for (const record of chronological(records)) {
    const advert = record.event.data;
    const key = bleStation(advert) ?? bleAnonymousKey(advert);
    const previous = devices.get(key);
    devices.set(key, {
      key,
      name: advert.name ?? previous?.name ?? null,
      vendor: bleVendor(advert) ?? previous?.vendor ?? null,
      beacon:
        advert.beacon == null ? (previous?.beacon ?? null) : BLE_BEACON_LABELS[advert.beacon.type],
      kind: BLE_PDU_LABELS[advert.pdu],
      channel: advert.channel ?? null,
      levelDbfs: advert.level_dbfs,
      heard: (previous?.heard ?? 0) + 1 + (advert.repeats ?? 0),
      at: record.at,
    });
  }
  return [...devices.values()].toSorted((a, b) => Date.parse(b.at) - Date.parse(a.at));
}
