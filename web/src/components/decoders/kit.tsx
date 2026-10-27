import type { ReactNode } from "react";
import { SONDE_BURST_M } from "../../lib/limits";
import type {
  BroadcastStatus,
  ChannelParams,
  LoraCodingRate,
  ParamChoice,
  ParamLimit,
  SondeType,
  SstvMode,
} from "../../lib/types";
import {
  type ChannelTypeId,
  choiceOptions,
  choicesOf,
  limitOf,
  type NumberLimit,
  type Settled,
  scaledLimit,
  unsetAs,
} from "../channelSettings";
import type { Options } from "../controls";
import {
  ChipField,
  ChoiceChip,
  NumberChip,
  OptionalNumberChip,
  SettingChip,
  ToggleChip,
} from "../face/Chips";
import { formatHz } from "../format";
import { LoraKeysChip } from "../LoraKeysChip";
import {
  CTCSS_DEFAULT_HZ,
  DCS_DEFAULT_CODE,
  INVERSION_DEFAULT_HZ,
  LORA_CUSTOM,
  LORA_PRESET_OPTIONS,
  loraPresetOf,
  withLoraPreset,
} from "../modeOptions";
import { NumberField } from "../NumberField";
import { Segmented } from "../Segmented";
import { withCurrent } from "../selectOptions";
import { TextAutocomplete } from "../TextAutocomplete";
import { WmbusKeysChip } from "../WmbusKeysChip";

export type {
  BroadcastStatus,
  ChannelParams,
  ChannelTypeId,
  LoraCodingRate,
  NumberLimit,
  Options,
  ParamChoice,
  ParamLimit,
  ReactNode,
  SondeType,
  SstvMode,
};
export {
  ChipField,
  ChoiceChip,
  CTCSS_DEFAULT_HZ,
  choiceOptions,
  choicesOf,
  DCS_DEFAULT_CODE,
  formatHz,
  INVERSION_DEFAULT_HZ,
  LORA_CUSTOM,
  LORA_PRESET_OPTIONS,
  LoraKeysChip,
  limitOf,
  loraPresetOf,
  NumberChip,
  NumberField,
  OptionalNumberChip,
  Segmented,
  SettingChip,
  SONDE_BURST_M,
  scaledLimit,
  TextAutocomplete,
  ToggleChip,
  unsetAs,
  WmbusKeysChip,
  withCurrent,
  withLoraPreset,
};

export type Limits = readonly ParamLimit[];
export type Choices = readonly ParamChoice[];

export const SSTV_AUTO = "auto";
export const SONDE_AUTO = "auto";

export type ParamsOf<K extends ChannelTypeId> = Extract<ChannelParams, { type: K }>;

export interface Mode<K extends ChannelTypeId> {
  params: Settled<ParamsOf<K>>;
  limits: Limits;
  choices: Choices;
  onParams: (params: ChannelParams) => void;
}

export interface BroadcastMode<K extends ChannelTypeId> extends Mode<K> {
  broadcast?: BroadcastStatus;
}

export const INVERT_TITLE = "Flip the signal's polarity; try it when nothing decodes";
export const SERVICE_TITLE =
  "Select a discovered audio, video or data service; Auto chooses the first playable service";

export function bandwidthOptions(valueHz: number, optionsHz: readonly number[]): Options<number> {
  return withCurrent(
    valueHz,
    optionsHz.map((hz) => ({ value: hz, label: formatHz(hz) })),
    formatHz,
  );
}

export function BandwidthChip({
  label = "BW",
  title = "Channel bandwidth",
  valueHz,
  optionsHz,
  onCommit,
}: {
  label?: string;
  title?: string;
  valueHz: number;
  optionsHz: readonly number[];
  onCommit: (hz: number) => void;
}) {
  return (
    <ChoiceChip
      label={label}
      title={title}
      value={valueHz}
      options={bandwidthOptions(valueHz, optionsHz)}
      onChange={onCommit}
    />
  );
}

export function HzChip({
  label,
  title,
  value,
  limit,
  onCommit,
}: {
  label: string;
  title: string;
  value: number;
  limit: NumberLimit;
  onCommit: (hz: number) => void;
}) {
  return (
    <NumberChip
      label={label}
      title={title}
      value={value}
      shown={formatHz(value)}
      unit="Hz"
      {...limit}
      onCommit={onCommit}
    />
  );
}

export function InvertChip({ on, onChange }: { on: boolean; onChange: (on: boolean) => void }) {
  return <ToggleChip label="Invert" title={INVERT_TITLE} on={on} onChange={onChange} />;
}

export function PresetChip({
  label,
  title,
  value,
  presets,
  limit,
  unit,
  onCommit,
}: {
  label: string;
  title: string;
  value: number;
  presets: Options<number>;
  limit: NumberLimit;
  unit?: string;
  onCommit: (value: number) => void;
}) {
  return (
    <SettingChip label={label} value={String(value)} unit={unit} title={title}>
      {() => (
        <ChipField label={title}>
          <Segmented
            label={`${title} presets`}
            value={value}
            options={presets}
            onChange={onCommit}
          />
          <NumberField
            className="min-w-0 flex-1"
            label={title}
            value={value}
            unit={unit}
            {...limit}
            onCommit={onCommit}
          />
        </ChipField>
      )}
    </SettingChip>
  );
}

export function serviceOptions(
  services: readonly { id: number; label: string }[],
  value: number | null,
): Options<string> {
  const options = [
    { value: "", label: "Auto" },
    ...services.map((service) => ({
      value: String(service.id),
      label: service.label || String(service.id),
    })),
  ];
  if (value !== null && !services.some((service) => service.id === value)) {
    options.push({ value: String(value), label: String(value) });
  }
  return options;
}

export function ServiceChip({
  status,
  value,
  max,
  onChange,
}: {
  status?: BroadcastStatus;
  value: number | null;
  max: number;
  onChange: (id: number | null) => void;
}) {
  const services = status?.services ?? [];
  if (services.length === 0) {
    return (
      <OptionalNumberChip
        label="Service"
        title={SERVICE_TITLE}
        placeholder="Auto"
        value={value}
        min={0}
        max={max}
        step={1}
        onCommit={onChange}
      />
    );
  }
  return (
    <ChoiceChip
      label="Service"
      title={SERVICE_TITLE}
      value={value === null ? "" : String(value)}
      options={serviceOptions(services, value)}
      quiet={value === null}
      onChange={(id) => onChange(id === "" ? null : Number(id))}
    />
  );
}
