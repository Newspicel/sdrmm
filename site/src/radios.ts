export interface FreqRange {
  low: number;
  high: number;
}

export type DriverKind = "built-in" | "vendor" | "soapy";
export type ConnectKind = "usb" | "network" | "both";
export type DcSpike = "no" | "corrected" | "small" | "some" | "yes" | "unknown";

export interface RadioSpec {
  id: string;
  name: string;
  shortLabel?: string;
  href: string;
  connect: ConnectKind;
  driver: DriverKind;
  ranges: readonly FreqRange[];
  rangeNote?: string;
  shortRange?: string;
  maxRateHz: number;
  rateNote?: string;
  bandwidthHz: number;
  bandwidthNote?: string;
  bits: number;
  rx: number;
  tx: number;
  dcSpike: DcSpike;
}

export const RADIOS: readonly RadioSpec[] = [
  {
    id: "rtlsdr",
    name: "RTL-SDR",
    shortLabel: "RTL-SDR",
    href: "/docs/radios/usb#rtl-sdr",
    connect: "usb",
    driver: "built-in",
    ranges: [{ low: 24e6, high: 1.766e9 }],
    shortRange: "24 MHz–1.77 GHz",
    maxRateHz: 3.2e6,
    bandwidthHz: 3.2e6,
    bits: 8,
    rx: 1,
    tx: 0,
    dcSpike: "small",
  },
  {
    id: "rtlsdr-v4",
    name: "RTL-SDR Blog V4",
    href: "/docs/radios/usb#rtl-sdr",
    connect: "usb",
    driver: "built-in",
    ranges: [{ low: 500e3, high: 1.766e9 }],
    shortRange: "500 kHz–1.77 GHz",
    maxRateHz: 3.2e6,
    bandwidthHz: 3.2e6,
    bits: 8,
    rx: 1,
    tx: 0,
    dcSpike: "small",
  },
  {
    id: "krakensdr",
    name: "KrakenSDR (5 lanes)",
    href: "/docs/radios/usb#krakensdr",
    connect: "usb",
    driver: "built-in",
    ranges: [{ low: 24e6, high: 1.766e9 }],
    shortRange: "24 MHz–1.77 GHz",
    maxRateHz: 2.56e6,
    bandwidthHz: 2.56e6,
    bits: 8,
    rx: 5,
    tx: 0,
    dcSpike: "small",
  },
  {
    id: "hackrf",
    name: "HackRF One",
    shortLabel: "HackRF",
    href: "/docs/radios/usb#hackrf",
    connect: "usb",
    driver: "built-in",
    ranges: [{ low: 1e6, high: 6e9 }],
    shortRange: "1 MHz–6 GHz",
    maxRateHz: 20e6,
    bandwidthHz: 20e6,
    bits: 8,
    rx: 1,
    tx: 1,
    dcSpike: "yes",
  },
  {
    id: "airspy-r2",
    name: "Airspy R2",
    shortLabel: "Airspy R2",
    href: "/docs/radios/usb#airspy",
    connect: "usb",
    driver: "built-in",
    ranges: [{ low: 24e6, high: 1.8e9 }],
    shortRange: "24 MHz–1.8 GHz",
    maxRateHz: 10e6,
    bandwidthHz: 9e6,
    bits: 12,
    rx: 1,
    tx: 0,
    dcSpike: "no",
  },
  {
    id: "airspy-mini",
    name: "Airspy Mini",
    href: "/docs/radios/usb#airspy",
    connect: "usb",
    driver: "built-in",
    ranges: [{ low: 24e6, high: 1.8e9 }],
    shortRange: "24 MHz–1.8 GHz",
    maxRateHz: 10e6,
    bandwidthHz: 6e6,
    bits: 12,
    rx: 1,
    tx: 0,
    dcSpike: "no",
  },
  {
    id: "airspy-hf",
    name: "Airspy HF+ Discovery",
    shortLabel: "Airspy HF+",
    href: "/docs/radios/usb#airspy",
    connect: "usb",
    driver: "built-in",
    ranges: [
      { low: 1e3, high: 31e6 },
      { low: 60e6, high: 260e6 },
    ],
    shortRange: "1 k–31 M, 60–260 M",
    maxRateHz: 768e3,
    bandwidthHz: 660e3,
    bits: 16,
    rx: 1,
    tx: 0,
    dcSpike: "some",
  },
  {
    id: "funcube",
    name: "FUNcube Dongle Pro+",
    shortLabel: "FUNcube+",
    href: "/docs/radios/usb#funcube-dongle-pro",
    connect: "usb",
    driver: "built-in",
    ranges: [
      { low: 150e3, high: 260e6 },
      { low: 410e6, high: 2.05e9 },
    ],
    shortRange: "150 k–260 M, 0.4–2 G",
    maxRateHz: 192e3,
    bandwidthHz: 192e3,
    bits: 16,
    rx: 1,
    tx: 0,
    dcSpike: "yes",
  },
  {
    id: "espsdr",
    name: "ESP32 (ESP-SDR)",
    shortLabel: "ESP-SDR",
    href: "/docs/radios/usb#esp-sdr",
    connect: "both",
    driver: "built-in",
    ranges: [
      { low: 2.4e9, high: 2.5e9 },
      { low: 5.15e9, high: 5.85e9 },
    ],
    rangeNote: "2.4 GHz (5 GHz on C5)",
    shortRange: "2.4 / 5 GHz",
    maxRateHz: 80e6,
    rateNote: "Bursts over serial",
    bandwidthHz: 69e6,
    bits: 10,
    rx: 1,
    tx: 0,
    dcSpike: "yes",
  },
  {
    id: "antsdr",
    name: "AntSDR E200 / E310",
    href: "/docs/radios/usb#antsdr",
    connect: "both",
    driver: "built-in",
    ranges: [{ low: 70e6, high: 6e9 }],
    shortRange: "70 MHz–6 GHz",
    maxRateHz: 61.44e6,
    rateNote: "~20 MS/s with iqlinkd",
    bandwidthHz: 56e6,
    bits: 12,
    rx: 2,
    tx: 2,
    dcSpike: "yes",
  },
  {
    id: "pluto",
    name: "ADALM-Pluto",
    shortLabel: "Pluto",
    href: "/docs/radios/usb#plutosdr-and-other-ad936x-boards",
    connect: "both",
    driver: "built-in",
    ranges: [{ low: 325e6, high: 3.8e9 }],
    shortRange: "325 MHz–3.8 GHz",
    maxRateHz: 61.44e6,
    rateNote: "~4 MS/s over USB",
    bandwidthHz: 20e6,
    bits: 12,
    rx: 1,
    tx: 1,
    dcSpike: "yes",
  },
  {
    id: "rsp1",
    name: "SDRplay RSP1 / RSP2",
    href: "/docs/radios/usb#sdrplay",
    connect: "usb",
    driver: "vendor",
    ranges: [{ low: 10e3, high: 2e9 }],
    shortRange: "10 kHz–2 GHz",
    maxRateHz: 10.66e6,
    bandwidthHz: 8e6,
    bits: 12,
    rx: 1,
    tx: 0,
    dcSpike: "corrected",
  },
  {
    id: "rspdx",
    name: "SDRplay RSP1A / RSP1B / RSPdx",
    shortLabel: "SDRplay 14b",
    href: "/docs/radios/usb#sdrplay",
    connect: "usb",
    driver: "vendor",
    ranges: [{ low: 1e3, high: 2e9 }],
    shortRange: "1 kHz–2 GHz",
    maxRateHz: 10.66e6,
    bandwidthHz: 8e6,
    bits: 14,
    rx: 1,
    tx: 0,
    dcSpike: "corrected",
  },
  {
    id: "rspduo",
    name: "SDRplay RSPduo",
    href: "/docs/radios/usb#sdrplay",
    connect: "usb",
    driver: "vendor",
    ranges: [{ low: 1e3, high: 2e9 }],
    shortRange: "1 kHz–2 GHz",
    maxRateHz: 10.66e6,
    bandwidthHz: 8e6,
    bandwidthNote: "1.536 MHz each in Dual Tuner",
    bits: 14,
    rx: 2,
    tx: 0,
    dcSpike: "corrected",
  },
  {
    id: "aaronia-plus",
    name: "Aaronia SPECTRAN V6 PLUS",
    shortLabel: "V6 PLUS",
    href: "/docs/radios/usb#aaronia-spectran-v6",
    connect: "usb",
    driver: "vendor",
    ranges: [{ low: 10e6, high: 6e9 }],
    rangeNote: "10 MHz to 6 GHz (8 GHz opt.)",
    shortRange: "10 MHz–6/8 GHz",
    maxRateHz: 245.76e6,
    rateNote: "Up to 491.52 MS/s on 2000XA",
    bandwidthHz: 160e6,
    bandwidthNote: "80 to 245 MHz per model",
    bits: 16,
    rx: 1,
    tx: 1,
    dcSpike: "unknown",
  },
  {
    id: "aaronia-eco",
    name: "Aaronia SPECTRAN V6 ECO",
    shortLabel: "V6 ECO",
    href: "/docs/radios/usb#aaronia-spectran-v6",
    connect: "usb",
    driver: "vendor",
    ranges: [{ low: 9e3, high: 6e9 }],
    rangeNote: "9 kHz to 6 GHz (per model)",
    shortRange: "9 kHz–6 GHz",
    maxRateHz: 61.44e6,
    bandwidthHz: 40e6,
    bits: 16,
    rx: 1,
    tx: 1,
    dcSpike: "unknown",
  },
  {
    id: "kiwisdr",
    name: "KiwiSDR",
    shortLabel: "KiwiSDR",
    href: "/docs/radios/network#kiwisdr",
    connect: "network",
    driver: "built-in",
    ranges: [{ low: 1e3, high: 30e6 }],
    rangeNote: "0 to 30 MHz",
    shortRange: "0–30 MHz",
    maxRateHz: 20e3,
    bandwidthHz: 20e3,
    bits: 14,
    rx: 1,
    tx: 0,
    dcSpike: "no",
  },
  {
    id: "cr8",
    name: "Dragon Labs CR-8",
    href: "/docs/radios/usb#dragon-labs-cr-8",
    connect: "usb",
    driver: "vendor",
    ranges: [{ low: 24e6, high: 1.766e9 }],
    shortRange: "24 MHz–1.77 GHz",
    maxRateHz: 12.5e6,
    bandwidthHz: 8e6,
    bits: 12,
    rx: 8,
    tx: 0,
    dcSpike: "unknown",
  },
  {
    id: "bladerf",
    name: "bladeRF 2.0 micro",
    shortLabel: "bladeRF / USRP",
    href: "/docs/radios/usb#soapysdr",
    connect: "usb",
    driver: "soapy",
    ranges: [{ low: 70e6, high: 6e9 }],
    shortRange: "70 MHz–6 GHz",
    maxRateHz: 61.44e6,
    bandwidthHz: 56e6,
    bits: 12,
    rx: 2,
    tx: 2,
    dcSpike: "yes",
  },
  {
    id: "limesdr",
    name: "LimeSDR USB",
    shortLabel: "LimeSDR",
    href: "/docs/radios/usb#soapysdr",
    connect: "usb",
    driver: "soapy",
    ranges: [{ low: 100e3, high: 3.8e9 }],
    shortRange: "100 kHz–3.8 GHz",
    maxRateHz: 61.44e6,
    bandwidthHz: 61.44e6,
    bits: 12,
    rx: 2,
    tx: 2,
    dcSpike: "yes",
  },
  {
    id: "limesdr-mini",
    name: "LimeSDR Mini 2.0",
    shortLabel: "Lime Mini",
    href: "/docs/radios/usb#soapysdr",
    connect: "usb",
    driver: "soapy",
    ranges: [{ low: 10e6, high: 3.5e9 }],
    shortRange: "10 MHz–3.5 GHz",
    maxRateHz: 30.72e6,
    bandwidthHz: 30.72e6,
    bits: 12,
    rx: 1,
    tx: 1,
    dcSpike: "yes",
  },
  {
    id: "usrp-b200",
    name: "USRP B200 / B205mini",
    href: "/docs/radios/usb#soapysdr",
    connect: "usb",
    driver: "soapy",
    ranges: [{ low: 70e6, high: 6e9 }],
    shortRange: "70 MHz–6 GHz",
    maxRateHz: 61.44e6,
    bandwidthHz: 56e6,
    bits: 12,
    rx: 1,
    tx: 1,
    dcSpike: "yes",
  },
  {
    id: "usrp-b210",
    name: "USRP B210",
    href: "/docs/radios/usb#soapysdr",
    connect: "usb",
    driver: "soapy",
    ranges: [{ low: 70e6, high: 6e9 }],
    shortRange: "70 MHz–6 GHz",
    maxRateHz: 61.44e6,
    bandwidthHz: 56e6,
    bits: 12,
    rx: 2,
    tx: 2,
    dcSpike: "yes",
  },
];

export const FREQ_MIN = 1e3;
export const FREQ_MAX = 6e9;
export const BW_MIN = 20e3;
export const BW_MAX = 250e6;

export const BAND_ZONES = [
  { label: "LF · MF", low: 1e3, high: 3e6, shaded: false },
  { label: "HF", low: 3e6, high: 30e6, shaded: true },
  { label: "VHF", low: 30e6, high: 300e6, shaded: false },
  { label: "UHF", low: 300e6, high: 3e9, shaded: true },
  { label: "", low: 3e9, high: 6e9, shaded: false },
] as const;

export const SPECTRUM_TICKS = [
  { label: "1 kHz", hz: 1e3 },
  { label: "100k", hz: 100e3 },
  { label: "1M", hz: 1e6 },
  { label: "10M", hz: 10e6 },
  { label: "100M", hz: 100e6 },
  { label: "1G", hz: 1e9 },
  { label: "6 GHz", hz: 6e9 },
] as const;

export const BW_TICKS = [
  { label: "20 kHz", hz: 20e3 },
  { label: "200 kHz", hz: 200e3 },
  { label: "2 MHz", hz: 2e6 },
  { label: "20 MHz", hz: 20e6 },
  { label: "250 MHz", hz: 250e6 },
] as const;

export function logPos(value: number, min: number, max: number): number {
  const clamped = Math.min(max, Math.max(min, value));
  return Math.log10(clamped / min) / Math.log10(max / min);
}

export function fmtHz(hz: number, suffix = "Hz"): string {
  if (hz >= 1e9) {
    return `${Number((hz / 1e9).toFixed(3))} G${suffix}`;
  }
  if (hz >= 1e6) {
    return `${Number((hz / 1e6).toFixed(2))} M${suffix}`;
  }
  if (hz >= 1e3) {
    return `${Number((hz / 1e3).toFixed(1))} k${suffix}`;
  }
  return `${hz} ${suffix}`;
}

export function fmtCompactHz(hz: number, suffix = "Hz"): string {
  if (hz >= 1e9) {
    return `${Number((hz / 1e9).toFixed(2))} G${suffix}`;
  }
  if (hz >= 1e6) {
    return `${Number((hz / 1e6).toFixed(1))} M${suffix}`;
  }
  if (hz >= 1e3) {
    return `${Number((hz / 1e3).toFixed(0))} k${suffix}`;
  }
  return `${hz} ${suffix}`;
}

export function fmtRanges(radio: RadioSpec): string {
  if (radio.rangeNote) {
    return radio.rangeNote;
  }
  return radio.ranges.map((r) => `${fmtHz(r.low)} to ${fmtHz(r.high)}`).join(", ");
}

export function fmtShortRanges(radio: RadioSpec): string {
  if (radio.shortRange) {
    return radio.shortRange;
  }
  return radio.ranges.map((r) => `${fmtCompactHz(r.low)}–${fmtCompactHz(r.high)}`).join(", ");
}

export function coversBand(radio: RadioSpec, hz: number): boolean {
  return radio.ranges.some((r) => r.low <= hz && hz <= r.high);
}

export function lowestHz(radio: RadioSpec): number {
  return Math.min(...radio.ranges.map((r) => r.low));
}

export function highestHz(radio: RadioSpec): number {
  return Math.max(...radio.ranges.map((r) => r.high));
}

export type RadioFilter = "builtin" | "tx" | "multi" | "hf" | "ghz24";

export function matchesRadio(
  radio: RadioSpec,
  query: string,
  filters: ReadonlySet<RadioFilter>,
): boolean {
  const q = query.trim().toLowerCase();
  if (q !== "" && !radio.name.toLowerCase().includes(q) && !radio.id.includes(q)) {
    return false;
  }
  if (filters.has("builtin") && radio.driver !== "built-in") return false;
  if (filters.has("tx") && radio.tx === 0) return false;
  if (filters.has("multi") && radio.rx < 2) return false;
  if (filters.has("hf") && !radio.ranges.some((r) => r.low <= 3e6)) return false;
  if (filters.has("ghz24") && !coversBand(radio, 2.45e9)) return false;
  return true;
}

export const DC_LABELS: Record<DcSpike, string> = {
  no: "No",
  corrected: "Corrected",
  small: "Small",
  some: "Some rates",
  yes: "Yes",
  unknown: "Unknown",
};

export const DRIVER_LABELS: Record<DriverKind, string> = {
  "built-in": "Built-in",
  vendor: "Vendor API",
  soapy: "SoapySDR",
};
