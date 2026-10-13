import { formatHz } from "../components/format";
import type { FieldValue, LatLon, Unit } from "../lib/types";

const SUFFIX: Readonly<Record<Unit, string>> = {
  none: "",
  hz: "Hz",
  db: "dB",
  dbfs: "dBFS",
  degrees: "°",
  feet: "ft",
  metres: "m",
  kilometres: "km",
  knots: "kn",
  metres_per_second: "m/s",
  feet_per_minute: "ft/min",
  celsius: "°C",
  percent: "%",
  hectopascal: "hPa",
  seconds: "s",
  baud: "Bd",
  millimetres: "mm",
  volts: "V",
  db_hz: "dB-Hz",
  kilometres_per_hour: "km/h",
  watts_per_square_metre: "W/m²",
  chips: "chips",
  wpm: "WPM",
  dbm: "dBm",
  psi: "psi",
  lux: "lx",
  kelvin: "K",
};

const TIGHT: ReadonlySet<Unit> = new Set<Unit>(["degrees", "percent"]);

export function formatNumber(value: number, unit: Unit, digits: number): string {
  if (unit === "hz") {
    return formatHz(value);
  }
  const text = value.toFixed(digits);
  const suffix = SUFFIX[unit];
  if (suffix === "") {
    return text;
  }
  return TIGHT.has(unit) ? `${text}${suffix}` : `${text} ${suffix}`;
}

export function formatLatLon(at: LatLon): string {
  return `${at.lat.toFixed(5)}, ${at.lon.toFixed(5)}`;
}

export function formatTime(at: string): string {
  const parsed = Date.parse(at);
  return Number.isNaN(parsed) ? at : new Date(parsed).toISOString().slice(0, 19).replace("T", " ");
}

export function formatValue(value: FieldValue): string {
  switch (value.type) {
    case "text":
      return value.value;
    case "number":
      return formatNumber(value.value.value, value.value.unit, value.value.digits);
    case "flag":
      return value.value ? "Yes" : "No";
    case "time":
      return formatTime(value.value);
    case "position":
      return formatLatLon(value.value);
  }
}
