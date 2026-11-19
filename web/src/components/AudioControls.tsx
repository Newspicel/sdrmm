import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Download, Plus, RotateCw, Trash2, X } from "lucide-react";
import {
  DENOISE_MODELS_KEY,
  deleteDenoiseModel,
  denoiseModelsQuery,
  downloadDenoiseModel,
} from "../lib/api";
import type {
  AudioAgcMode,
  AudioProcessing,
  DenoiseModel,
  DenoiseModelStatus,
  NotchSettings,
} from "../lib/types";
import { Button } from "./BaseControls";
import { Checkbox } from "./Checkbox";
import {
  AUDIO_DEFAULTS,
  AUDIO_LIMITS,
  mergeAudio,
  withNotchAdded,
  withNotchAt,
  withNotchRemoved,
} from "./channelSettings";
import { CHIP_SETTING, ICON_BTN_SM, type Options } from "./controls";
import { ChipField, ChoiceChip, SettingChip, ToggleChip } from "./face/Chips";
import { formatBytes, formatHz } from "./format";
import { Icon } from "./Icon";
import { NumberField } from "./NumberField";
import { rowKeys } from "./rowKeys";
import { Select } from "./Select";
import { SliderField } from "./Slider";
import { useDebouncedCommit } from "./useDebouncedCommit";

const AGC_MODES: Options<AudioAgcMode> = [
  { value: "off", label: "off" },
  { value: "slow", label: "Slow" },
  { value: "medium", label: "Med" },
  { value: "fast", label: "Fast" },
];

type DenoiseChoice = "off" | "spectral" | DenoiseModel;

const DENOISE_CHOICES: Options<DenoiseChoice> = [
  { value: "off", label: "Off" },
  { value: "spectral", label: "Spectral", title: "Light and fast, for steady hiss" },
  { value: "dpdfnet2_8khz", label: "DPDFNet 2", title: "Speech model, light. Downloaded once" },
  { value: "dpdfnet8_8khz", label: "DPDFNet 8", title: "Speech model, best, more CPU" },
];

function denoiseChoice(denoise: NonNullable<AudioProcessing["denoise"]>): DenoiseChoice {
  if (!(denoise.enabled ?? false)) {
    return "off";
  }
  return (denoise.mode ?? "spectral") === "neural"
    ? (denoise.model ?? "dpdfnet2_8khz")
    : "spectral";
}

function withDenoiseChoice(
  denoise: NonNullable<AudioProcessing["denoise"]>,
  choice: DenoiseChoice,
): NonNullable<AudioProcessing["denoise"]> {
  if (choice === "off") {
    return { ...denoise, enabled: false };
  }
  if (choice === "spectral") {
    return { ...denoise, enabled: true, mode: "spectral" };
  }
  return { ...denoise, enabled: true, mode: "neural", model: choice };
}

type Edit = (patch: Partial<AudioProcessing>) => void;

export function AudioControls({
  audio,
  onAudio,
}: {
  audio: AudioProcessing;
  onAudio: (audio: AudioProcessing) => void;
}) {
  const notches = audio.notches ?? [];
  const notchKeys = rowKeys(notches);
  const edit: Edit = (patch) => onAudio(mergeAudio(audio, patch));
  const agc = audio.agc ?? "off";
  return (
    <>
      <ChoiceChip
        label="AGC"
        title="Audio AGC speed"
        value={agc}
        options={AGC_MODES}
        quiet={agc === "off"}
        onChange={(next) => edit({ agc: next })}
      />
      <DeclickChip audio={audio} edit={edit} />
      <DenoiseChip audio={audio} edit={edit} />
      <ToggleChip
        label="Auto notch"
        title="Finds and removes steady carriers"
        on={audio.auto_notch ?? false}
        onChange={(auto_notch) => edit({ auto_notch })}
      />
      <PassbandChip audio={audio} edit={edit} />
      <PeakChip audio={audio} edit={edit} />
      {notches.map((notch, index) => (
        <NotchChip
          key={notchKeys[index]}
          index={index}
          notch={notch}
          onEdit={(patch) => edit({ notches: withNotchAt(notches, index, patch) })}
          onRemove={() => edit({ notches: withNotchRemoved(notches, index) })}
        />
      ))}
      <Button
        type="button"
        className={CHIP_SETTING}
        disabled={notches.length >= AUDIO_LIMITS.maxNotches}
        title={`Add a notch, up to ${AUDIO_LIMITS.maxNotches}`}
        onClick={() => {
          const next = withNotchAdded(notches);
          if (next !== null) {
            edit({ notches: next });
          }
        }}
      >
        <Icon glyph={Plus} size={12} />
        <span className="font-sans">Notch</span>
      </Button>
    </>
  );
}

function DeclickChip({ audio, edit }: { audio: AudioProcessing; edit: Edit }) {
  const clicks = audio.click_removal ?? {};
  const enabled = clicks.enabled ?? false;
  const threshold = clicks.threshold ?? AUDIO_DEFAULTS.clickThreshold;
  const slider = useDebouncedCommit((next: number) =>
    edit({ click_removal: { ...clicks, threshold: next } }),
  );
  const shown = slider.pending ?? threshold;
  return (
    <SettingChip
      label="De-click"
      value={enabled ? `${threshold.toFixed(1)}×` : "off"}
      quiet={!enabled}
      title="Click removal"
    >
      {() => (
        <ChipField label="Click removal">
          <Checkbox
            label="Click removal"
            checked={enabled}
            onChange={(next) => edit({ click_removal: { ...clicks, enabled: next } })}
          />
          <SliderField
            label="Click threshold"
            disabled={!enabled}
            min={AUDIO_LIMITS.clickThreshold.min}
            max={AUDIO_LIMITS.clickThreshold.max}
            step={0.5}
            value={shown}
            onChange={slider.change}
            readout={
              <>
                {shown.toFixed(1)}
                <span className="text-ink-faint">×</span>
              </>
            }
          />
        </ChipField>
      )}
    </SettingChip>
  );
}

function DenoiseChip({ audio, edit }: { audio: AudioProcessing; edit: Edit }) {
  const denoise = audio.denoise ?? {};
  const choice = denoiseChoice(denoise);
  const enabled = choice !== "off";
  const neural = enabled && choice !== "spectral";
  const strength = denoise.strength ?? AUDIO_DEFAULTS.denoiseStrength;
  const models = useQuery(denoiseModelsQuery(neural));
  const status = models.data?.models.find((entry) => entry.model === choice);
  const missing = neural && status !== undefined && status.state !== "ready";
  const slider = useDebouncedCommit((next: number) =>
    edit({ denoise: { ...denoise, strength: next } }),
  );
  const shown = slider.pending ?? strength;
  return (
    <SettingChip
      label="Denoise"
      value={!enabled ? "off" : missing ? "no model" : `${Math.round(strength * 100)}%`}
      quiet={!enabled}
      tone={missing ? "danger" : undefined}
      title="Noise reduction"
    >
      {() => (
        <>
          <ChipField label="Denoise">
            <Select
              label="Noise reduction"
              value={choice}
              options={DENOISE_CHOICES}
              className="w-full"
              onChange={(next) => edit({ denoise: withDenoiseChoice(denoise, next) })}
            />
            {neural && status !== undefined && <ModelState status={status} />}
          </ChipField>
          {enabled && (
            <ChipField label="Strength">
              <SliderField
                label="Noise reduction strength"
                min={0}
                max={1}
                step={0.05}
                value={shown}
                onChange={slider.change}
                readout={
                  <>
                    {Math.round(shown * 100)}
                    <span className="text-ink-faint">%</span>
                  </>
                }
              />
            </ChipField>
          )}
        </>
      )}
    </SettingChip>
  );
}

function ModelState({ status }: { status: DenoiseModelStatus }) {
  const queryClient = useQueryClient();
  const refresh = () => void queryClient.invalidateQueries({ queryKey: DENOISE_MODELS_KEY });
  const download = useMutation({ mutationFn: downloadDenoiseModel, onSettled: refresh });
  const remove = useMutation({ mutationFn: deleteDenoiseModel, onSettled: refresh });
  const size = formatBytes(status.bytes);
  switch (status.state) {
    case "ready":
      return (
        <Button
          type="button"
          className={`${ICON_BTN_SM} hover:text-danger`}
          aria-label="Remove model"
          title={`Remove model, frees ${size}`}
          onClick={() => remove.mutate(status.model)}
        >
          <Icon glyph={Trash2} size={12} />
        </Button>
      );
    case "downloading":
      return (
        <span className="legend tabular-nums">
          {Math.floor((status.received / Math.max(status.bytes, 1)) * 100)}%
        </span>
      );
    case "failed":
      return (
        <Button
          type="button"
          className={`${CHIP_SETTING} text-danger`}
          title={status.error}
          onClick={() => download.mutate(status.model)}
        >
          <Icon glyph={RotateCw} size={12} />
          <span className="font-sans">Retry</span>
        </Button>
      );
    case "missing":
      return (
        <Button
          type="button"
          className={CHIP_SETTING}
          title="Download model"
          disabled={download.isPending}
          onClick={() => download.mutate(status.model)}
        >
          <Icon glyph={Download} size={12} />
          <span className="font-sans">{size}</span>
        </Button>
      );
  }
}

function PassbandChip({ audio, edit }: { audio: AudioProcessing; edit: Edit }) {
  const filter = audio.filter ?? {};
  const enabled = filter.enabled ?? false;
  const lowHz = filter.low_hz ?? AUDIO_DEFAULTS.filterLowHz;
  const highHz = filter.high_hz ?? AUDIO_DEFAULTS.filterHighHz;
  const invalid = lowHz >= highHz;
  return (
    <SettingChip
      label="Passband"
      value={enabled ? `${formatHz(lowHz)} – ${formatHz(highHz)}` : "off"}
      quiet={!enabled}
      tone={enabled && invalid ? "danger" : undefined}
      title="Audio filter"
    >
      {() => (
        <ChipField label="Audio filter">
          <Checkbox
            label="Audio filter"
            checked={enabled}
            onChange={(next) => edit({ filter: { ...filter, enabled: next } })}
          />
          <NumberField
            label="Audio filter low cut"
            unit="Hz"
            value={lowHz}
            min={AUDIO_LIMITS.toneHz.min}
            max={AUDIO_LIMITS.toneHz.max}
            step={10}
            invalid={invalid}
            className="w-20"
            onCommit={(low_hz) => edit({ filter: { ...filter, low_hz } })}
          />
          <span className="legend">–</span>
          <NumberField
            label="Audio filter high cut"
            unit="Hz"
            value={highHz}
            min={AUDIO_LIMITS.toneHz.min}
            max={AUDIO_LIMITS.toneHz.max}
            step={10}
            invalid={invalid}
            className="w-20"
            onCommit={(high_hz) => edit({ filter: { ...filter, high_hz } })}
          />
        </ChipField>
      )}
    </SettingChip>
  );
}

function PeakChip({ audio, edit }: { audio: AudioProcessing; edit: Edit }) {
  const peak = audio.peak ?? {};
  const enabled = peak.enabled ?? false;
  const freqHz = peak.freq_hz ?? AUDIO_DEFAULTS.peakFreqHz;
  const gainDb = peak.gain_db ?? AUDIO_DEFAULTS.peakGainDb;
  const slider = useDebouncedCommit((next: number) => edit({ peak: { ...peak, gain_db: next } }));
  const shown = slider.pending ?? gainDb;
  return (
    <SettingChip
      label="Peak"
      value={enabled ? formatHz(freqHz) : "off"}
      quiet={!enabled}
      title="Peak filter"
    >
      {() => (
        <ChipField label="Peak filter">
          <Checkbox
            label="Peak filter"
            checked={enabled}
            onChange={(next) => edit({ peak: { ...peak, enabled: next } })}
          />
          <NumberField
            label="Peak frequency"
            unit="Hz"
            value={freqHz}
            min={AUDIO_LIMITS.toneHz.min}
            max={AUDIO_LIMITS.toneHz.max}
            step={10}
            className="w-20"
            onCommit={(freq_hz) => edit({ peak: { ...peak, freq_hz } })}
          />
          <NumberField
            label="Peak width"
            unit="Hz"
            value={peak.width_hz ?? AUDIO_DEFAULTS.peakWidthHz}
            min={AUDIO_LIMITS.notchWidthHz.min}
            max={AUDIO_LIMITS.notchWidthHz.max}
            step={10}
            className="w-20"
            onCommit={(width_hz) => edit({ peak: { ...peak, width_hz } })}
          />
          <SliderField
            label="Peak gain"
            disabled={!enabled}
            min={AUDIO_LIMITS.peakGainDb.min}
            max={AUDIO_LIMITS.peakGainDb.max}
            step={1}
            value={shown}
            onChange={slider.change}
            readout={
              <>
                {shown.toFixed(0)}
                <span className="text-ink-faint">dB</span>
              </>
            }
          />
        </ChipField>
      )}
    </SettingChip>
  );
}

function NotchChip({
  index,
  notch,
  onEdit,
  onRemove,
}: {
  index: number;
  notch: NotchSettings;
  onEdit: (patch: Partial<NotchSettings>) => void;
  onRemove: () => void;
}) {
  const name = `Notch ${index + 1}`;
  const freqHz = notch.freq_hz ?? AUDIO_DEFAULTS.notchFreqHz;
  return (
    <SettingChip label={name} value={formatHz(freqHz)} title={name}>
      {(close) => (
        <NotchFields
          name={name}
          notch={notch}
          onEdit={onEdit}
          onRemove={() => {
            onRemove();
            close();
          }}
        />
      )}
    </SettingChip>
  );
}

export function NotchFields({
  name,
  notch,
  onEdit,
  onRemove,
}: {
  name: string;
  notch: NotchSettings;
  onEdit: (patch: Partial<NotchSettings>) => void;
  onRemove: () => void;
}) {
  return (
    <ChipField label={name}>
      <NumberField
        label={`${name} frequency`}
        value={notch.freq_hz ?? AUDIO_DEFAULTS.notchFreqHz}
        min={AUDIO_LIMITS.toneHz.min}
        max={AUDIO_LIMITS.toneHz.max}
        step={10}
        className="w-24"
        unit="Hz"
        onCommit={(freq_hz) => onEdit({ freq_hz })}
      />
      <NumberField
        label={`${name} width`}
        value={notch.width_hz ?? AUDIO_DEFAULTS.notchWidthHz}
        min={AUDIO_LIMITS.notchWidthHz.min}
        max={AUDIO_LIMITS.notchWidthHz.max}
        step={10}
        className="w-24"
        unit="Hz"
        onCommit={(width_hz) => onEdit({ width_hz })}
      />
      <Button
        type="button"
        className={`${ICON_BTN_SM} ml-auto hover:text-danger`}
        aria-label={`Remove ${name.toLowerCase()}`}
        onClick={onRemove}
      >
        <Icon glyph={X} size={12} />
      </Button>
    </ChipField>
  );
}
