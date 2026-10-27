import {
  ChoiceChip,
  choiceOptions,
  choicesOf,
  LORA_CUSTOM,
  LORA_PRESET_OPTIONS,
  type LoraCodingRate,
  LoraKeysChip,
  loraPresetOf,
  type Mode,
  OptionalNumberChip,
  type ParamsOf,
  ToggleChip,
  withLoraPreset,
} from "./kit";

export function LoraChips({ params, choices, onParams }: Mode<"lora">) {
  const settings = params.settings;
  const set = (next: ParamsOf<"lora">["settings"]) => onParams({ type: "lora", settings: next });
  const preset = loraPresetOf(settings);
  const spreadingFactor = settings.spreading_factor;
  const protocol = settings.protocol;
  const header = settings.implicit_header ?? null;
  return (
    <>
      <ChoiceChip
        label="Preset"
        title="Modem preset; frequency stays as tuned"
        value={preset}
        options={LORA_PRESET_OPTIONS}
        quiet={preset === LORA_CUSTOM}
        onChange={(id) => set(withLoraPreset(settings, id))}
      />
      <ChoiceChip
        label="BW"
        title="LoRa bandwidth"
        value={settings.bandwidth}
        options={choicesOf(choices, params.settings, "bandwidth")}
        onChange={(bandwidth) => set({ ...settings, bandwidth })}
      />
      <ChoiceChip
        label="SF"
        title="Spreading factor"
        value={spreadingFactor}
        options={choicesOf(choices, params.settings, "spreading_factor")}
        quiet={spreadingFactor === "all"}
        onChange={(spreading_factor) => set({ ...settings, spreading_factor })}
      />
      <ChoiceChip
        label="IQ"
        title="Chirp direction"
        value={settings.iq}
        options={choicesOf(choices, params.settings, "iq")}
        onChange={(iq) => set({ ...settings, iq })}
      />
      <ChoiceChip
        label="Protocol"
        title="Payload protocol"
        value={protocol}
        options={choicesOf(choices, params.settings, "protocol")}
        quiet={protocol === "auto"}
        onChange={(next) => set({ ...settings, protocol: next })}
      />
      <LoraKeysChip keys={settings.keys ?? []} onChange={(keys) => set({ ...settings, keys })} />
      <OptionalNumberChip
        label="Implicit"
        title="Implicit header payload length, empty for explicit headers"
        placeholder="off"
        value={header?.length ?? null}
        min={1}
        max={255}
        step={1}
        unit="B"
        onCommit={(length) =>
          set({
            ...settings,
            implicit_header:
              length === null
                ? null
                : { coding_rate: "4/5", crc: true, ...header, length: Math.round(length) },
          })
        }
      />
      {header !== null && (
        <>
          <ChoiceChip
            label="CR"
            title="Implicit header coding rate"
            value={header.coding_rate ?? "4/5"}
            options={choiceOptions<LoraCodingRate>(choices, "implicit_header.coding_rate")}
            onChange={(coding_rate) =>
              set({ ...settings, implicit_header: { ...header, coding_rate } })
            }
          />
          <ToggleChip
            label="CRC"
            title="Implicit header payload carries a CRC"
            on={header.crc ?? true}
            onChange={(crc) => set({ ...settings, implicit_header: { ...header, crc } })}
          />
        </>
      )}
    </>
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

function LoraRow({ station, now }: { station: LoraStation; now: number }) {
  const ageMs = now - Date.parse(station.at);
  return (
    <tr className={`border-b border-line/40 ${ageClass(ageMs)}`}>
      <td className={`${TABLE_CELL} font-mono`}>{station.key}</td>
      <td className={TABLE_CELL}>{station.name ?? "-"}</td>
      <td className={TABLE_CELL}>{station.protocol}</td>
      <td className={`${TABLE_CELL} max-w-64 truncate`} title={station.message}>
        {station.message}
      </td>
      <td className={`${TABLE_CELL} tabular-nums`}>{station.snrDb.toFixed(1)}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>{station.frames}</td>
      <td className={`${TABLE_CELL} text-right`}>{formatAge(ageMs)}</td>
    </tr>
  );
}

export function LoraView({ records }: PanelProps<"lora">) {
  const now = useNow(1000);
  const stations = loraStations(records);
  if (stations.length === 0) {
    return (
      <div className={PANE}>
        <span className={EMPTY}>No LoRa nodes heard yet.</span>
      </div>
    );
  }
  return (
    <div className={PANE}>
      <div className="overflow-x-auto">
        <table className="w-full border-collapse text-left text-xs">
          <thead>
            <tr className="border-b border-line">
              <th className={TABLE_HEAD}>Node</th>
              <th className={TABLE_HEAD}>Name</th>
              <th className={TABLE_HEAD}>Protocol</th>
              <th className={TABLE_HEAD}>Last</th>
              <th className={TABLE_HEAD}>
                SNR <Unit symbol="dB" />
              </th>
              <th className={TABLE_HEAD}>Frames</th>
              <th className={TABLE_HEAD}>Heard</th>
            </tr>
          </thead>
          <tbody>
            {stations.map((station) => (
              <LoraRow key={station.key} station={station} now={now} />
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

import { loraMessage, loraName, loraProtocol, loraStation } from "../../lib/lora";
import { type DecodedRecordOf } from "../../lib/types";
import { chronological } from "../decoderViews";

export interface LoraStation {
  key: string;
  protocol: string;
  name: string | null;
  message: string;
  snrDb: number;
  frames: number;
  at: string;
}

export function loraStations(records: readonly DecodedRecordOf<"lora">[]): LoraStation[] {
  const stations = new Map<string, LoraStation>();
  for (const record of chronological(records)) {
    const frame = record.event.data;
    const key = loraStation(frame);
    if (key === null) {
      continue;
    }
    const previous = stations.get(key);
    stations.set(key, {
      key,
      protocol: loraProtocol(frame),
      name: loraName(frame) ?? previous?.name ?? null,
      message: loraMessage(frame),
      snrDb: frame.snr_db,
      frames: (previous?.frames ?? 0) + 1,
      at: record.at,
    });
  }
  return [...stations.values()].toSorted((a, b) => Date.parse(b.at) - Date.parse(a.at));
}
