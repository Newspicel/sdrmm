import type { SondeType } from "../lib/types";
import type { ChannelParamsOf } from "./channelSettings";
import type { Options } from "./controls";
import { LRPT_MODE_LABELS, SONDE_LABELS } from "./weatherFormat";

export const DMR_SLOTS: Options<NonNullable<ChannelParamsOf<"dmr">["slots"]>> = [
  { value: "both", label: "Both" },
  { value: "one", label: "TS1" },
  { value: "two", label: "TS2" },
];
export const NXDN_WIDTHS: Options<NonNullable<ChannelParamsOf<"nxdn">["bandwidth"]>> = [
  { value: "narrow", label: "6.25" },
  { value: "wide", label: "12.5" },
];
export const BLE_LINKS: Options<NonNullable<ChannelParamsOf<"ble">["link"]>> = [
  { value: "channel", label: "Channel", title: "One Bluetooth channel at 4 MS/s" },
  { value: "band", label: "Band", title: "Every Bluetooth channel in a 20 MS/s window" },
];
export const AIRTIME_SPANS: Options<NonNullable<ChannelParamsOf<"wifi_occupancy">["span"]>> = [
  { value: "mhz20", label: "20", title: "20 MS/s" },
  { value: "mhz40", label: "40", title: "40 MS/s" },
  { value: "mhz80", label: "80", title: "80 MS/s, all of 2.4 GHz" },
];
export const REMOTE_ID_LINKS: Options<NonNullable<ChannelParamsOf<"remote_id">["link"]>> = [
  { value: "bluetooth", label: "Bluetooth", title: "One Bluetooth channel at 4 MS/s" },
  {
    value: "bluetooth_band",
    label: "BT band",
    title: "Every Bluetooth channel in a 20 MS/s window, for long range",
  },
  { value: "wifi", label: "Wi-Fi", title: "One 20 MHz Wi-Fi channel" },
];

export const DECT_BANDS: Options<NonNullable<ChannelParamsOf<"dect">["band"]>> = [
  { value: "eu", label: "EU" },
  { value: "us", label: "US" },
];
export const DECT_SPANS: Options<NonNullable<ChannelParamsOf<"dect">["span"]>> = [
  { value: "carrier", label: "Carrier", title: "One carrier at 2.304 MS/s" },
  {
    value: "band",
    label: "Band",
    title: "Every carrier at once. Tune to the band centre, radio at 20 MS/s (EU) or 10 MS/s (US)",
  },
];
export const DECT_SIDES: Options<NonNullable<ChannelParamsOf<"dect">["sides"]>> = [
  { value: "both", label: "Both" },
  { value: "rfp", label: "Base" },
  { value: "pp", label: "Handset" },
];
export const SIDEBANDS: Options<NonNullable<ChannelParamsOf<"ssb">["sideband"]>> = [
  { value: "usb", label: "USB" },
  { value: "lsb", label: "LSB" },
];
export const SELCALL_SYSTEMS: Options<NonNullable<ChannelParamsOf<"selcall">["system"]>> = [
  { value: "ccir1", label: "CCIR-1" },
  { value: "zvei1", label: "ZVEI-1" },
];
export const ILS_COMPONENTS: Options<NonNullable<ChannelParamsOf<"ils">["component"]>> = [
  { value: "localizer", label: "Localizer" },
  { value: "glideslope", label: "Glideslope" },
];
export const POCSAG_BAUDS: Options<NonNullable<ChannelParamsOf<"pocsag">["baud"]>> = [
  { value: "auto", label: "Auto" },
  { value: "b512", label: "512" },
  { value: "b1200", label: "1200" },
  { value: "b2400", label: "2400" },
];
export const AIS_CHANNELS: Options<NonNullable<ChannelParamsOf<"ais">["ais_channel"]>> = [
  { value: "a", label: "A" },
  { value: "b", label: "B" },
];
export const AERO_CHANNELS: Options<NonNullable<ChannelParamsOf<"inmarsat_aero">["channel"]>> = [
  { value: "p", label: "P", title: "Forward channel to aircraft" },
  { value: "burst", label: "R/T", title: "Bursts from aircraft" },
  { value: "c", label: "C", title: "Voice circuit" },
];
export const IRIDIUM_SPANS: Options<NonNullable<ChannelParamsOf<"iridium">["span"]>> = [
  { value: "channel", label: "50 kHz", title: "One channel" },
  { value: "mhz1", label: "1 MHz", title: "Radio at 1 MS/s, decodes the middle 800 kHz" },
  { value: "mhz2_5", label: "2.5 MHz", title: "Radio at 2.5 MS/s, decodes the middle 2 MHz" },
  { value: "mhz5", label: "5 MHz", title: "Radio at 5 MS/s, decodes the middle 4 MHz" },
  { value: "mhz10", label: "10 MHz", title: "Radio at 10 MS/s, decodes the middle 8 MHz" },
];
export const DVBT_BANDWIDTHS: Options<NonNullable<ChannelParamsOf<"dvbt">["bandwidth"]>> = [
  { value: "khz250", label: "250 kHz" },
  { value: "khz333", label: "333 kHz" },
  { value: "khz500", label: "500 kHz" },
  { value: "mhz1", label: "1 MHz" },
  { value: "mhz1_7", label: "1.7 MHz" },
  { value: "mhz2", label: "2 MHz" },
  { value: "mhz5", label: "5 MHz" },
  { value: "mhz6", label: "6 MHz" },
  { value: "mhz7", label: "7 MHz" },
  { value: "mhz8", label: "8 MHz" },
  { value: "mhz10", label: "10 MHz" },
];
export const APRS_MODES: Options<NonNullable<ChannelParamsOf<"aprs">["mode"]>> = [
  { value: "afsk1200", label: "AFSK 1200" },
  { value: "g3ruh9600", label: "G3RUH 9600" },
];
export const NFM_TONE_MODES: Options<NonNullable<ChannelParamsOf<"nfm">["tone_mode"]>> = [
  { value: "off", label: "Off" },
  { value: "detect", label: "Detect" },
  { value: "ctcss", label: "CTCSS" },
  { value: "dcs", label: "DCS" },
];
export const NFM_SCRAMBLER_MODES: Options<NonNullable<ChannelParamsOf<"nfm">["scrambler_mode"]>> = [
  { value: "off", label: "Off" },
  { value: "inversion", label: "Inversion" },
  { value: "auto", label: "Auto" },
];
const CTCSS_TONES_HZ = [
  67.0, 69.3, 71.9, 74.4, 77.0, 79.7, 82.5, 85.4, 88.5, 91.5, 94.8, 97.4, 100.0, 103.5, 107.2,
  110.9, 114.8, 118.8, 123.0, 127.3, 131.8, 136.5, 141.3, 146.2, 151.4, 156.7, 159.8, 162.2, 165.5,
  167.9, 171.3, 173.8, 177.3, 179.9, 183.5, 186.2, 189.9, 192.8, 196.6, 199.5, 203.5, 206.5, 210.7,
  218.1, 225.7, 229.1, 233.6, 241.8, 250.3, 254.1,
];
const DCS_CODES = [
  23, 25, 26, 31, 32, 43, 47, 51, 54, 65, 71, 72, 73, 74, 114, 115, 116, 125, 131, 132, 134, 143,
  152, 155, 156, 162, 165, 172, 174, 205, 223, 226, 243, 244, 245, 251, 261, 263, 265, 271, 306,
  311, 315, 331, 343, 346, 351, 364, 365, 371, 411, 412, 413, 423, 431, 432, 445, 464, 465, 466,
  503, 506, 516, 532, 546, 565, 606, 612, 624, 627, 631, 632, 654, 662, 664, 703, 712, 723, 731,
  732, 734, 743, 754,
];
export const CTCSS_DEFAULT_HZ = 88.5;
export const INVERSION_DEFAULT_HZ = 3_300;
export const DCS_DEFAULT_CODE = 23;
export const CTCSS_OPTIONS: Options<number> = CTCSS_TONES_HZ.map((hz) => ({
  value: hz,
  label: `${hz.toFixed(1)} Hz`,
}));
export const DCS_OPTIONS: Options<number> = DCS_CODES.map((code) => ({
  value: code,
  label: String(code).padStart(3, "0"),
}));
export const RTTY_STOP_BITS: Options<NonNullable<ChannelParamsOf<"rtty">["stop_bits"]>> = [
  { value: "one", label: "1" },
  { value: "one_and_half", label: "1.5" },
  { value: "two", label: "2" },
];
export const ATV_MODULATIONS: Options<NonNullable<ChannelParamsOf<"atv">["modulation"]>> = [
  { value: "am", label: "AM" },
  { value: "fm", label: "FM" },
];
export const ATV_STANDARDS: Options<NonNullable<ChannelParamsOf<"atv">["standard"]>> = [
  { value: "ccir625", label: "625 / 25" },
  { value: "eia525", label: "525 / 30" },
  { value: "system_a405", label: "405 / 25" },
];
export const DAB_MODES: Options<NonNullable<ChannelParamsOf<"dab">["mode"]>> = [
  { value: "auto", label: "Auto" },
  { value: "dab", label: "DAB" },
  { value: "dab_plus", label: "DAB+" },
];
export const DAB_TRANSMISSION_MODES: Options<
  NonNullable<ChannelParamsOf<"dab">["transmission_mode"]>
> = [
  { value: "auto", label: "Auto" },
  { value: "i", label: "I" },
  { value: "ii", label: "II" },
  { value: "iii", label: "III" },
  { value: "iv", label: "IV" },
];
export const DATV_STANDARDS: Options<NonNullable<ChannelParamsOf<"datv">["standard"]>> = [
  { value: "dvb_s", label: "DVB-S" },
  { value: "dvb_s2", label: "DVB-S2" },
];
export const DATV_ROLL_OFFS: Options<NonNullable<ChannelParamsOf<"datv">["roll_off"]>> = [
  { value: "pct35", label: "0.35" },
  { value: "pct25", label: "0.25" },
  { value: "pct20", label: "0.20" },
  { value: "pct15", label: "0.15" },
  { value: "pct10", label: "0.10" },
  { value: "pct5", label: "0.05" },
];
export const DRM_MODES: Options<NonNullable<ChannelParamsOf<"drm">["mode"]>> = [
  { value: "auto", label: "Auto" },
  { value: "drm30", label: "DRM30" },
  { value: "drm_plus", label: "DRM+" },
];
export const SSTV_MODULATIONS: Options<NonNullable<ChannelParamsOf<"sstv">["modulation"]>> = [
  { value: "usb", label: "USB" },
  { value: "lsb", label: "LSB" },
  { value: "fm", label: "FM" },
  { value: "am", label: "AM" },
];
export const SSTV_AUTO = "auto";
export const SSTV_MODES: Options<NonNullable<ChannelParamsOf<"sstv">["mode"]> | typeof SSTV_AUTO> =
  [
    { value: SSTV_AUTO, label: "Follow VIS" },
    { value: "robot36", label: "Robot 36" },
    { value: "robot72", label: "Robot 72" },
    { value: "martin_m1", label: "Martin M1" },
    { value: "martin_m2", label: "Martin M2" },
    { value: "scottie_s1", label: "Scottie S1" },
    { value: "scottie_s2", label: "Scottie S2" },
    { value: "scottie_dx", label: "Scottie DX" },
    { value: "pd50", label: "PD50" },
    { value: "pd90", label: "PD90" },
    { value: "pd120", label: "PD120" },
    { value: "pd180", label: "PD180" },
    { value: "sc2180", label: "Wraase SC2-180" },
  ];
export const ATV_COLORS: Options<NonNullable<ChannelParamsOf<"atv">["color"]>> = [
  { value: "monochrome", label: "Mono" },
  { value: "pal", label: "PAL" },
  { value: "ntsc", label: "NTSC" },
];
export const DEEMPHASIS_US: Options<number> = [
  { value: 50, label: "50 µs" },
  { value: 75, label: "75 µs" },
];
export const RTTY_BAUDS: Options<number> = [
  { value: 45.45, label: "45.45" },
  { value: 50, label: "50" },
  { value: 75, label: "75" },
];
export const RTTY_SHIFTS_HZ: Options<number> = [
  { value: 170, label: "170" },
  { value: 450, label: "450" },
  { value: 850, label: "850" },
];
export const PSK_BAUDS: Options<NonNullable<ChannelParamsOf<"psk">["baud"]>> = [
  { value: "psk31", label: "PSK31" },
  { value: "psk63", label: "PSK63" },
  { value: "psk125", label: "PSK125" },
  { value: "psk250", label: "PSK250" },
];

export const RADIO_CLOCK_STANDARDS: Options<
  NonNullable<ChannelParamsOf<"radio_clock">["standard"]>
> = [
  { value: "dcf77", label: "DCF77" },
  { value: "wwvb", label: "WWVB" },
  { value: "msf", label: "MSF" },
  { value: "jjy", label: "JJY" },
];

export const DATV_CODE_RATES: Options<NonNullable<ChannelParamsOf<"datv">["code_rate"]>> = [
  { value: "auto", label: "Auto" },
  { value: "half", label: "1/2" },
  { value: "two_thirds", label: "2/3" },
  { value: "three_quarters", label: "3/4" },
  { value: "five_sixths", label: "5/6" },
  { value: "seven_eighths", label: "7/8" },
];
export const DVBT_STANDARDS: Options<NonNullable<ChannelParamsOf<"dvbt">["standard"]>> = [
  { value: "dvb_t", label: "DVB-T" },
  { value: "dvb_t2", label: "DVB-T2" },
];
function labelled<V extends string>(labels: Record<V, string>): Options<V> {
  return (Object.keys(labels) as V[]).map((value) => ({ value, label: labels[value] }));
}

export const LRPT_MODES = labelled(LRPT_MODE_LABELS);
export const WEFAX_IOCS: Options<NonNullable<ChannelParamsOf<"wefax">["ioc"]>> = [
  { value: "ioc576", label: "576" },
  { value: "ioc288", label: "288" },
];
export const WEFAX_LPMS: Options<NonNullable<ChannelParamsOf<"wefax">["lpm"]>> = [
  { value: "lpm60", label: "60" },
  { value: "lpm90", label: "90" },
  { value: "lpm120", label: "120" },
  { value: "lpm240", label: "240" },
];
export const SONDE_AUTO = "auto";
export const SONDE_TYPES: Options<SondeType | typeof SONDE_AUTO> = [
  { value: SONDE_AUTO, label: "Auto" },
  ...labelled(SONDE_LABELS),
];

type LoraSettings = ChannelParamsOf<"lora">;
export type LoraKey = NonNullable<LoraSettings["keys"]>[number];
export type LoraKeyKind = LoraKey["kind"];

export const LORA_BANDWIDTHS: Options<NonNullable<LoraSettings["bandwidth"]>> = [
  { value: "khz7_8", label: "7.8 kHz" },
  { value: "khz10_4", label: "10.4 kHz" },
  { value: "khz15_6", label: "15.6 kHz" },
  { value: "khz20_8", label: "20.8 kHz" },
  { value: "khz31_25", label: "31.25 kHz" },
  { value: "khz41_7", label: "41.7 kHz" },
  { value: "khz62_5", label: "62.5 kHz" },
  { value: "khz125", label: "125 kHz" },
  { value: "khz250", label: "250 kHz" },
  { value: "khz500", label: "500 kHz" },
];
export const LORA_SPREADING_FACTORS: Options<NonNullable<LoraSettings["spreading_factor"]>> = [
  { value: "all", label: "All", title: "Search SF7 to SF12 at once" },
  { value: "sf7", label: "SF7" },
  { value: "sf8", label: "SF8" },
  { value: "sf9", label: "SF9" },
  { value: "sf10", label: "SF10" },
  { value: "sf11", label: "SF11" },
  { value: "sf12", label: "SF12" },
];
export const LORA_IQ: Options<NonNullable<LoraSettings["iq"]>> = [
  { value: "normal", label: "Normal" },
  { value: "inverted", label: "Inverted", title: "LoRaWAN downlinks" },
  { value: "both", label: "Both" },
];
export const LORA_PROTOCOLS: Options<NonNullable<LoraSettings["protocol"]>> = [
  { value: "auto", label: "Auto" },
  { value: "raw", label: "LoRa", title: "Bytes only" },
  { value: "lorawan", label: "LoRaWAN" },
  { value: "meshtastic", label: "Meshtastic" },
  { value: "meshcore", label: "MeshCore" },
];
export const LORA_CODING_RATES: Options<
  NonNullable<NonNullable<LoraSettings["implicit_header"]>["coding_rate"]>
> = [
  { value: "4/5", label: "4/5" },
  { value: "4/6", label: "4/6" },
  { value: "4/7", label: "4/7" },
  { value: "4/8", label: "4/8" },
];

type LoraPresetSettings = Required<
  Pick<LoraSettings, "bandwidth" | "spreading_factor" | "protocol" | "iq">
>;

interface LoraPreset {
  id: string;
  label: string;
  title: string;
  settings: LoraPresetSettings;
}

function meshtastic(
  label: string,
  bandwidth: LoraPresetSettings["bandwidth"],
  spreading_factor: LoraPresetSettings["spreading_factor"],
): LoraPreset {
  return {
    id: `meshtastic_${label}`,
    label,
    title: "Meshtastic modem preset",
    settings: { bandwidth, spreading_factor, protocol: "meshtastic", iq: "normal" },
  };
}

function meshcore(
  label: string,
  spreading_factor: LoraPresetSettings["spreading_factor"],
): LoraPreset {
  return {
    id: `meshcore_${label}`,
    label: `MeshCore ${label}`,
    title: "MeshCore narrow preset",
    settings: { bandwidth: "khz62_5", spreading_factor, protocol: "meshcore", iq: "normal" },
  };
}

export const LORA_PRESETS: readonly LoraPreset[] = [
  meshtastic("LongFast", "khz250", "sf11"),
  meshtastic("LongModerate", "khz125", "sf11"),
  meshtastic("LongSlow", "khz125", "sf12"),
  meshtastic("MediumSlow", "khz250", "sf10"),
  meshtastic("MediumFast", "khz250", "sf9"),
  meshtastic("ShortSlow", "khz250", "sf8"),
  meshtastic("ShortFast", "khz250", "sf7"),
  meshtastic("ShortTurbo", "khz500", "sf7"),
  meshcore("EU", "sf8"),
  meshcore("US", "sf7"),
  {
    id: "lorawan",
    label: "LoRaWAN",
    title: "Uplinks and downlinks on one 125 kHz channel",
    settings: { bandwidth: "khz125", spreading_factor: "all", protocol: "lorawan", iq: "both" },
  },
];

export const LORA_CUSTOM = "custom";

export const LORA_PRESET_OPTIONS: Options<string> = [
  ...LORA_PRESETS.map(({ id, label, title }) => ({ value: id, label, title })),
  { value: LORA_CUSTOM, label: "Custom", disabled: true },
];

export function loraPresetOf(settings: LoraSettings): string {
  const current: LoraPresetSettings = {
    bandwidth: settings.bandwidth ?? "khz125",
    spreading_factor: settings.spreading_factor ?? "all",
    protocol: settings.protocol ?? "auto",
    iq: settings.iq ?? "normal",
  };
  const match = LORA_PRESETS.find((preset) =>
    (Object.keys(current) as (keyof LoraPresetSettings)[]).every(
      (field) => preset.settings[field] === current[field],
    ),
  );
  return match?.id ?? LORA_CUSTOM;
}

export function withLoraPreset(settings: LoraSettings, id: string): LoraSettings {
  const preset = LORA_PRESETS.find((candidate) => candidate.id === id);
  return preset === undefined ? settings : { ...settings, ...preset.settings };
}

export const LORA_KEY_KINDS: Options<LoraKeyKind> = [
  { value: "meshtastic_channel", label: "Meshtastic", title: "Channel name and base64 PSK" },
  { value: "meshcore_channel", label: "MeshCore", title: "Channel name and hex secret" },
  { value: "lorawan_session", label: "ABP", title: "LoRaWAN DevAddr, NwkSKey and AppSKey" },
  { value: "lorawan_app_key", label: "OTAA", title: "LoRaWAN AppKey" },
];

export const LORA_KEY_FIELDS: Record<LoraKeyKind, readonly (readonly [string, string])[]> = {
  meshtastic_channel: [
    ["name", "Channel"],
    ["psk", "PSK base64"],
  ],
  meshcore_channel: [
    ["name", "Channel or #tag"],
    ["secret", "Secret hex"],
  ],
  lorawan_session: [
    ["dev_addr", "DevAddr"],
    ["nwk_s_key", "NwkSKey"],
    ["app_s_key", "AppSKey"],
  ],
  lorawan_app_key: [["app_key", "AppKey"]],
};

export function blankLoraKey(kind: LoraKeyKind): LoraKey {
  switch (kind) {
    case "meshtastic_channel":
      return { kind, name: "", psk: "" };
    case "meshcore_channel":
      return { kind, name: "", secret: "" };
    case "lorawan_session":
      return { kind, dev_addr: "", nwk_s_key: "", app_s_key: "" };
    case "lorawan_app_key":
      return { kind, app_key: "" };
  }
}

export function loraKeyText(key: LoraKey, field: string): string {
  return (key as Record<string, string>)[field] ?? "";
}

export function withLoraKeyText(key: LoraKey, field: string, text: string): LoraKey {
  return { ...key, [field]: text };
}

export function loraKeyComplete(key: LoraKey): boolean {
  switch (key.kind) {
    case "meshtastic_channel":
      return key.name !== "";
    case "meshcore_channel":
      return key.secret !== "" ? key.name !== "" : /^#./.test(key.name);
    case "lorawan_session":
      return key.dev_addr !== "" && key.nwk_s_key !== "" && key.app_s_key !== "";
    case "lorawan_app_key":
      return key.app_key !== "";
  }
}
