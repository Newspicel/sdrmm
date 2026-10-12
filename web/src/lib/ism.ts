import {
  bearing,
  celsius,
  metresPerSecond,
  millimetres,
  percent,
} from "../components/weatherFormat";
import type { IsmCoding, IsmIntegrity, IsmMessage, IsmModulation } from "./types";

export const ISM_UNKNOWN_MODEL = "Unknown";

export const ISM_MODULATION_LABELS: Record<IsmModulation, string> = {
  ook: "OOK",
  fsk: "FSK",
};

export const ISM_CODING_LABELS: Record<IsmCoding, string> = {
  pwm: "PWM",
  ppm: "PPM",
  pcm: "PCM",
  manchester: "Manchester",
};

export const ISM_INTEGRITY_LABELS: Record<IsmIntegrity, string> = {
  crc: "CRC",
  checksum: "checksum",
  repeats: "repeats",
};

export function ismStation(m: IsmMessage): string {
  return `${m.model} ${m.id}`;
}

function onOff(value: boolean | null | undefined, on: string, off: string | null): string | null {
  return value == null ? null : value ? on : off;
}

export function ismReadings(m: IsmMessage): (string | undefined | null)[] {
  return [
    celsius(m.temperature_c),
    celsius(m.temperature_2_c),
    percent(m.humidity_pct),
    m.pressure_kpa == null ? null : `${Math.round(m.pressure_kpa)} kPa`,
    metresPerSecond(m.wind_avg_m_s),
    m.wind_max_m_s == null ? null : `gust ${metresPerSecond(m.wind_max_m_s)}`,
    bearing(m.wind_dir_deg),
    m.rain_mm == null ? null : `rain ${millimetres(m.rain_mm)}`,
    m.uv_index == null ? null : `UV ${m.uv_index.toFixed(1)}`,
    m.light_lux == null ? null : `${Math.round(m.light_lux)} lx`,
    onOff(m.contact_open, "open", "closed"),
    onOff(m.tamper, "tamper", null),
    onOff(m.alarm, "alarm", null),
    m.command == null ? null : `cmd ${m.command}`,
  ];
}

export function ismSummary(m: IsmMessage): string {
  return [
    ismStation(m),
    m.channel == null ? null : `ch ${m.channel}`,
    ...ismReadings(m),
    m.model === ISM_UNKNOWN_MODEL ? `${m.bits} bits` : null,
    m.battery_ok === false ? "battery low" : null,
  ]
    .filter((part) => part != null)
    .join(" · ");
}
