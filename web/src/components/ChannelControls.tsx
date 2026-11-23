import { statusChoices } from "../lib/channelStatus";
import type { ChannelBand, ChannelDescriptor, ChannelSettings, ChannelStatus } from "../lib/types";
import type { ChannelEdit } from "../lib/useChannelPatch";
import { BlankerChip } from "./BlankerControl";
import {
  AUDIO_DEFAULTS,
  AUDIO_LIMITS,
  channelFrequencyHz,
  channelHasAudio,
  paramsWith,
  radioWindowHz,
  settledParams,
  squelchMarginDb,
} from "./channelSettings";
import { inTuningRange, type Range } from "./dial";
import { FrequencyDial } from "./FrequencyDial";
import { Chips, NumberChip } from "./face/Chips";
import { formatMhz } from "./format";
import { IfFilterChip } from "./IfFilterControl";
import { SettingsForm } from "./SettingsForm";
import { TuneTo } from "./TuneTo";
import { TuningLock } from "./TuningLock";
import { Unused } from "./Unused";

export function ChannelDial({
  hz,
  band,
  spanHz,
  centerHz,
  range,
  dialId,
  wheelTunes,
  locked: lockedByHand,
  heldBy,
  onTune,
  onLock,
}: {
  hz: number;
  band: ChannelBand | null;
  spanHz: number | null;
  centerHz: number | null;
  range: Range;
  dialId: string;
  wheelTunes: boolean;
  locked: boolean;
  heldBy: string | null;
  onTune: (hz: number) => void;
  onLock: (locked: boolean) => void;
}) {
  const heard = radioWindowHz(centerHz, spanHz, band);
  const locked = lockedByHand || heldBy !== null;
  return (
    <div className="flex min-w-0 items-center gap-1">
      <FrequencyDial
        id={dialId}
        hz={hz}
        range={range}
        disabled={locked}
        wheelTunes={wheelTunes}
        onTune={onTune}
      />
      <span className="ml-auto flex shrink-0 items-center gap-1">
        <TuneTo
          title="Type a frequency to listen on"
          hz={hz}
          hint={
            heard === null
              ? `Reaches ${formatMhz(range.min)} – ${formatMhz(range.max)}`
              : `The radio hears ${formatMhz(heard.lowHz)} – ${formatMhz(heard.highHz)}`
          }
          resolve={(entered) => inTuningRange(entered, range)}
          disabled={locked}
          onTune={onTune}
        />
        <TuningLock
          locked={locked}
          held="Frequency locked"
          free="Lock frequency"
          hold={heldBy === null ? null : `Tuned by ${heldBy}. Unwire its control to tune by hand.`}
          onLock={onLock}
        />
      </span>
    </div>
  );
}

export function ChannelControls({
  settings,
  descriptor,
  band,
  onEdit,
  status,
}: {
  settings: ChannelSettings;
  descriptor: ChannelDescriptor | undefined;
  band: ChannelBand | null;
  onEdit: (edit: ChannelEdit) => void;
  status?: ChannelStatus;
}) {
  const audio = channelHasAudio(descriptor);
  const params = settledParams(settings.params, descriptor?.defaults?.params);
  return (
    <Chips inset="none">
      <SettingsForm
        form={descriptor?.settings ?? []}
        settings={params.settings ?? {}}
        choices={statusChoices(status)}
        readouts={status?.readouts}
        onSettings={(next) => onEdit({ params: paramsWith(params.type, next) })}
      />
      {audio && (
        <BlankerChip
          blanker={settings.blanker ?? {}}
          onBlanker={(blanker) => onEdit({ blanker })}
        />
      )}
      {band !== null && (
        <IfFilterChip
          filter={settings.if_filter ?? {}}
          own={band}
          channelHz={channelFrequencyHz(settings)}
          onFilter={(if_filter) => onEdit({ if_filter })}
        />
      )}
    </Chips>
  );
}

/** The auto squelch margin: greyed until Squelch is on Auto, then how far above the noise it opens. */
export function SquelchMargin({
  settings,
  onEdit,
}: {
  settings: ChannelSettings;
  onEdit: (edit: ChannelEdit) => void;
}) {
  const marginDb = squelchMarginDb(settings.squelch);
  const { min, max } = AUDIO_LIMITS.squelchAutoMarginDb;
  const chip = (
    <NumberChip
      label="Margin"
      value={marginDb ?? AUDIO_DEFAULTS.squelchAutoMarginDb}
      unit="dB"
      min={min}
      max={max}
      step={1}
      title="How far above the measured noise floor the squelch opens"
      onCommit={(margin_db) => onEdit({ squelch: { mode: "auto", margin_db } })}
    />
  );
  return marginDb === null ? <Unused needs={AUTO_SQUELCH}>{chip}</Unused> : chip;
}

const AUTO_SQUELCH = { key: "squelch", hint: "Set Squelch to Auto" };
