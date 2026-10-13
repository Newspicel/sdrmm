import type { ChannelParamsOf } from "./channelSettings";
import type { Options } from "./controls";

export const CTCSS_DEFAULT_HZ = 88.5;
export const INVERSION_DEFAULT_HZ = 3_300;
export const DCS_DEFAULT_CODE = 23;

type LoraSettings = ChannelParamsOf<"lora">;
export type LoraKey = NonNullable<LoraSettings["keys"]>[number];
export type LoraKeyKind = LoraKey["kind"];

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
