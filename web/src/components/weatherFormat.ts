import type { AprsWeather, AptImage, SondeType } from "../lib/types";

type AvhrrChannel = NonNullable<AptImage["channel_a"]>;

export const SONDE_LABELS: Record<SondeType, string> = {
  rs41: "RS41",
  dfm: "DFM",
  m10: "M10",
  m20: "M20",
  imet4: "iMet-4",
};

export const AVHRR_LABELS: Record<AvhrrChannel, string> = {
  ch1: "1",
  ch2: "2",
  ch3a: "3A",
  ch3b: "3B",
  ch4: "4",
  ch5: "5",
};

type Maybe = number | null | undefined;

function unit(value: Maybe, digits: number, symbol: string): string | undefined {
  return value == null ? undefined : `${value.toFixed(digits)} ${symbol}`;
}

export function celsius(value: Maybe): string | undefined {
  return unit(value, 1, "°C");
}

export function percent(value: Maybe): string | undefined {
  return value == null ? undefined : `${Math.round(value)}%`;
}

export function hectopascal(value: Maybe): string | undefined {
  return unit(value, 1, "hPa");
}

export function metresPerSecond(value: Maybe): string | undefined {
  return unit(value, 1, "m/s");
}

export function climbRate(value: Maybe): string | undefined {
  return value == null ? undefined : `${value >= 0 ? "+" : ""}${value.toFixed(1)} m/s`;
}

export function metres(value: Maybe): string | undefined {
  return value == null ? undefined : `${Math.round(value).toLocaleString("en-US")} m`;
}

export function millimetres(value: Maybe): string | undefined {
  return unit(value, 1, "mm");
}

export function bearing(value: Maybe): string | undefined {
  return value == null ? undefined : `${Math.round(value)}°`;
}

export function seconds(ms: number): string {
  return `${(ms / 1000).toFixed(1)} s`;
}

export function wind(weather: AprsWeather): string | undefined {
  const speed = metresPerSecond(weather.wind_speed_ms);
  if (speed === undefined) {
    return undefined;
  }
  const from = bearing(weather.wind_dir_deg);
  return from === undefined ? speed : `${speed} from ${from}`;
}
