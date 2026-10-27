import { BandwidthChip, ChoiceChip, choicesOf, type Mode } from "./kit";

export function AprsChips({ params, choices, onParams }: Mode<"aprs">) {
  const settings = params.settings;
  return (
    <>
      <ChoiceChip
        label="Mode"
        title="APRS mode"
        value={settings.mode}
        options={choicesOf(choices, params.settings, "mode")}
        onChange={(mode) => onParams({ type: "aprs", settings: { ...settings, mode } })}
      />
      <BandwidthChip
        valueHz={settings.bandwidth_hz}
        optionsHz={[12_500, 25_000]}
        onCommit={(bandwidth_hz) =>
          onParams({ type: "aprs", settings: { ...settings, bandwidth_hz } })
        }
      />
    </>
  );
}

import {
  ageClass,
  celsius,
  EMPTY,
  formatAge,
  hectopascal,
  metresPerSecond,
  millimetres,
  PANE,
  type PanelProps,
  percent,
  TABLE_CELL,
  TABLE_HEAD,
  useNow,
  wind,
} from "./panelKit";

function AprsWeatherRow({ station, now }: { station: AprsWeatherStation; now: number }) {
  const weather = station.weather;
  const ageMs = now - Date.parse(station.at);
  return (
    <tr className={`border-b border-line/40 ${ageClass(ageMs)}`}>
      <td className={`${TABLE_CELL} font-mono`}>{station.source}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>{celsius(weather.temperature_c) ?? "-"}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>
        {station.minTemperatureC === null
          ? "-"
          : `${celsius(station.minTemperatureC)} / ${celsius(station.maxTemperatureC)}`}
      </td>
      <td className={`${TABLE_CELL} tabular-nums`}>{wind(weather) ?? "-"}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>{metresPerSecond(station.maxGustMs) ?? "-"}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>{percent(weather.humidity_pct) ?? "-"}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>{hectopascal(weather.pressure_hpa) ?? "-"}</td>
      <td className={`${TABLE_CELL} tabular-nums`}>{millimetres(weather.rain_1h_mm) ?? "-"}</td>
      <td className={`${TABLE_CELL} text-right`}>{formatAge(ageMs)}</td>
    </tr>
  );
}

export function AprsWeatherView({ records }: PanelProps<"aprs">) {
  const now = useNow(1000);
  const stations = aprsWeatherStations(records);
  if (stations.length === 0) {
    return (
      <div className={PANE}>
        <span className={EMPTY}>No weather station heard yet.</span>
      </div>
    );
  }
  return (
    <div className={PANE}>
      <div className="overflow-x-auto">
        <table className="w-full min-w-[40rem] border-collapse text-left text-xs">
          <thead>
            <tr className="border-b border-line">
              <th className={TABLE_HEAD}>Station</th>
              <th className={TABLE_HEAD}>Temp</th>
              <th className={TABLE_HEAD} title="Lowest and highest temperature in the kept reports">
                Min / max
              </th>
              <th className={TABLE_HEAD}>Wind</th>
              <th className={TABLE_HEAD} title="Strongest gust in the kept reports">
                Gust
              </th>
              <th className={TABLE_HEAD} title="Relative humidity">
                RH
              </th>
              <th className={TABLE_HEAD}>Pressure</th>
              <th className={TABLE_HEAD} title="Rain in the last hour">
                Rain
              </th>
              <th className={TABLE_HEAD}>Heard</th>
            </tr>
          </thead>
          <tbody>
            {stations.map((station) => (
              <AprsWeatherRow key={station.source} station={station} now={now} />
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

import { type AprsWeather, type DecodedRecordOf } from "../../lib/types";
import { chronological, highest, lowest } from "../decoderViews";

export interface AprsWeatherStation {
  source: string;
  weather: AprsWeather;
  minTemperatureC: number | null;
  maxTemperatureC: number | null;
  maxGustMs: number | null;
  reports: number;
  at: string;
}

export function aprsWeatherStations(
  records: readonly DecodedRecordOf<"aprs">[],
): AprsWeatherStation[] {
  const stations = new Map<string, AprsWeatherStation>();
  for (const record of chronological(records)) {
    const weather = record.event.data.weather;
    if (weather == null) {
      continue;
    }
    const source = record.event.data.source;
    stations.set(source, nextWeather(stations.get(source), source, weather, record.at));
  }
  return [...stations.values()].toSorted((a, b) => Date.parse(b.at) - Date.parse(a.at));
}

export function nextWeather(
  previous: AprsWeatherStation | undefined,
  source: string,
  weather: AprsWeather,
  at: string,
): AprsWeatherStation {
  const temperature = weather.temperature_c ?? null;
  return {
    source,
    weather: { ...previous?.weather, ...withoutNulls(weather) },
    minTemperatureC: lowest(previous?.minTemperatureC ?? null, temperature),
    maxTemperatureC: highest(previous?.maxTemperatureC ?? null, temperature),
    maxGustMs: highest(previous?.maxGustMs ?? null, weather.wind_gust_ms ?? null),
    reports: (previous?.reports ?? 0) + 1,
    at,
  };
}

export function withoutNulls(weather: AprsWeather): AprsWeather {
  return Object.fromEntries(Object.entries(weather).filter(([, value]) => value != null));
}
