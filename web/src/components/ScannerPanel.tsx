import { useMutation, useQueryClient } from "@tanstack/react-query";
import { X } from "lucide-react";
import { FaceBody, FaceFooter } from "../canvas/nodes/NodeShell";
import { STATE_KEY, skipScan, startScan, stopScan } from "../lib/api";
import { decoderKey, useScannerStore } from "../lib/scanner";
import { pushToast } from "../lib/toasts";
import type {
  BandPlan,
  ChannelInfo,
  DeviceSet,
  ScanMode,
  ScannerStatus,
  ScanRange,
  ScanSettings,
} from "../lib/types";
import { useBandPlan } from "../lib/useBandPlan";
import { Button } from "./BaseControls";
import { BTN, BTN_DANGER, BTN_PRIMARY, BTN_SM, listItem } from "./controls";
import { ChipField, Chips, ChoiceChip, NumberChip, SettingChip, ToggleChip } from "./face/Chips";
import { FaceFault } from "./face/Fault";
import { Readout, Readouts } from "./face/Readouts";
import { FaceStats, Stat } from "./face/Stats";
import { Icon } from "./Icon";
import { NumberField, OptionalNumberField } from "./NumberField";
import {
  applyPreset,
  channelName,
  formatDb,
  formatMhz,
  liveStatus,
  MIN_STEP_KHZ,
  presetIn,
  rangeProblem,
  scanPresets,
  stateLabel,
  sweepKind,
  targetCount,
  withHz,
  withoutHz,
} from "./scanner";

const SCAN_MODES = [
  { value: "targets", label: "Listed", title: "Stop on the first busy frequency" },
  { value: "close_call", label: "Strongest", title: "Stop on the strongest signal nearby" },
  { value: "all", label: "All", title: "Visit every busy frequency once, then finish" },
] as const;

export function ScannerPanel({
  active,
  channel,
  hint,
  settings,
  onSettings,
}: {
  active: DeviceSet | null;
  channel: ChannelInfo | null;
  hint: string;
  settings: ScanSettings;
  onSettings: (next: ScanSettings) => void;
}) {
  const queryClient = useQueryClient();
  const { plan } = useBandPlan();
  const pushed = useScannerStore((s) =>
    active && channel ? s.byDecoder[decoderKey(active.id, channel.id)] : undefined,
  );
  const clearLive = useScannerStore((s) => s.clear);
  const status = liveStatus(active, channel?.id ?? null, pushed);
  const invalidate = (): void => void queryClient.invalidateQueries({ queryKey: STATE_KEY });
  const keepLockouts = (finished: ScannerStatus): void =>
    onSettings({ ...settings, lockouts: finished.settings.lockouts ?? [] });

  const startMut = useMutation({
    mutationFn: (target: { deviceSet: number; channel: number }) =>
      startScan(target, { ...settings, channel: target.channel }),
    onError: (e) => pushToast(e.message),
    onSettled: invalidate,
  });
  const stopMut = useMutation({
    mutationFn: stopScan,
    onSuccess: (finished, decoder) => {
      keepLockouts(finished);
      clearLive(decoder.deviceSet, decoder.channel);
    },
    onError: (e) => pushToast(e.message),
    onSettled: invalidate,
  });
  const skipMut = useMutation({
    mutationFn: skipScan,
    onSuccess: keepLockouts,
    onError: (e) => pushToast(e.message),
    onSettled: invalidate,
  });

  const problem = rangeProblem(settings);
  const busy = startMut.isPending || stopMut.isPending || skipMut.isPending;
  const decoder =
    active !== null && channel !== null ? { deviceSet: active.id, channel: channel.id } : null;
  return (
    <>
      <FaceBody title={active === null ? hint : undefined}>
        {status !== null ? (
          <>
            <ScanReadouts status={status} active={active} plan={plan} />
            {status.error != null && <FaceFault message={status.error} />}
          </>
        ) : (
          <>
            <Readouts ruled={false}>
              {channel !== null && (
                <Readout label="Feeds">
                  {channel.settings.params.type} at {formatMhz(channel.settings.frequency_hz)}
                </Readout>
              )}
              <Readout label="Sweep">{sweepKind(active, null)}</Readout>
            </Readouts>
            <ScanSetup
              settings={settings}
              onSettings={onSettings}
              plan={plan}
              firmwareSweep={active?.capabilities.hardware_sweep === true}
            />
            {problem !== null && <FaceFault message={problem} />}
          </>
        )}
      </FaceBody>

      <FaceFooter>
        {status !== null ? (
          <ScanStats status={status} />
        ) : (
          problem === null && (
            <FaceStats>
              <Stat label="Targets" title="Frequencies per sweep">
                {targetCount(settings)}
              </Stat>
            </FaceStats>
          )
        )}
        {status !== null && decoder !== null ? (
          <>
            <Button
              type="button"
              className={BTN}
              disabled={busy || status.state !== "holding"}
              title="Leave this frequency and lock it out"
              onClick={() => skipMut.mutate(decoder)}
            >
              Skip
            </Button>
            <Button
              type="button"
              className={status.state === "done" ? BTN : BTN_DANGER}
              disabled={busy}
              onClick={() => stopMut.mutate(decoder)}
            >
              {status.state === "done" ? "Close" : "Stop scan"}
            </Button>
          </>
        ) : (
          <>
            <Button
              type="button"
              className={BTN}
              onClick={() =>
                onSettings({
                  ...settings,
                  ranges: [...(settings.ranges ?? []), nextRange(settings)],
                })
              }
            >
              Add range
            </Button>
            <Button
              type="button"
              className={BTN_PRIMARY}
              disabled={decoder === null || busy || problem !== null}
              onClick={() => decoder !== null && startMut.mutate(decoder)}
            >
              Start scan
            </Button>
          </>
        )}
      </FaceFooter>
    </>
  );
}

function nextRange(settings: ScanSettings): ScanRange {
  const last = settings.ranges?.at(-1);
  return last ?? { start_hz: 145_600_000, stop_hz: 145_800_000, step_hz: 12_500 };
}

function ScanReadouts({
  status,
  active,
  plan,
}: {
  status: ScannerStatus;
  active: DeviceSet | null;
  plan: BandPlan | null;
}) {
  const name = channelName(plan, status.current_hz);
  const mode = SCAN_MODES.find((entry) => entry.value === (status.settings.mode ?? "targets"));
  return (
    <Readouts ruled={false}>
      <Readout label="State">
        <span className={status.state === "scanning" ? "" : "text-accent"}>
          {stateLabel(status)}
        </span>
      </Readout>
      <Readout label="Frequency">
        {formatMhz(status.current_hz)}
        {name !== null && ` ${name}`}
      </Readout>
      <Readout label="Over noise">{formatDb(status.current_snr_db)}</Readout>
      <Readout label="Find">{mode?.label ?? "-"}</Readout>
      <Readout label="Sweep">{sweepKind(active, status)}</Readout>
      <Readout label="Span">
        {formatMhz(status.first_hz)} to {formatMhz(status.last_hz)}
      </Readout>
    </Readouts>
  );
}

function ScanStats({ status }: { status: ScannerStatus }) {
  const locked = status.settings.lockouts?.length ?? 0;
  return (
    <FaceStats>
      <Stat label="Targets" title="Frequencies per sweep">
        {status.targets}
      </Stat>
      <Stat label="Sweeps" title="Sweeps finished">
        {status.sweeps}
      </Stat>
      <Stat label="Hits" title="Signals held">
        {status.hits}
      </Stat>
      {locked > 0 && (
        <Stat label="Locked" title="Frequencies locked out">
          {locked}
        </Stat>
      )}
    </FaceStats>
  );
}

function ScanSetup({
  settings,
  onSettings,
  plan,
  firmwareSweep,
}: {
  settings: ScanSettings;
  onSettings: (next: ScanSettings) => void;
  plan: BandPlan | null;
  firmwareSweep: boolean;
}) {
  const mode: ScanMode = settings.mode ?? "targets";
  const ranges = settings.ranges ?? [];
  const patch = (next: Partial<ScanSettings>): void => onSettings({ ...settings, ...next });
  const patchRange = (index: number, next: Partial<ScanRange>): void =>
    patch({ ranges: ranges.map((r, i) => (i === index ? { ...r, ...next } : r)) });
  return (
    <Chips className="border-t border-line p-2">
      <BandChip settings={settings} plan={plan} onSettings={onSettings} />
      {ranges.map((range, index) => (
        <RangeChip
          key={index}
          range={range}
          index={index}
          numbered={ranges.length > 1}
          onPatch={(next) => patchRange(index, next)}
          onRemove={() => patch({ ranges: ranges.filter((_, i) => i !== index) })}
        />
      ))}
      {(settings.frequencies ?? []).length > 0 && (
        <FrequencyListChip
          label="Channels"
          title="Listed channels"
          values={settings.frequencies ?? []}
          plan={plan}
          onChange={(frequencies) => patch({ frequencies })}
        />
      )}
      <ChoiceChip
        label="Find"
        title="Scan mode"
        value={mode}
        options={SCAN_MODES}
        onChange={(next) => patch({ mode: next })}
      />
      <NumberChip
        label="Over noise"
        title="How far over the noise floor counts as busy"
        unit="dB"
        value={settings.margin_db ?? 12}
        min={1}
        max={60}
        step={1}
        onCommit={(margin_db) => patch({ margin_db })}
      />
      {mode === "all" ? (
        <NumberChip
          label="Hold"
          title="Time on each busy frequency, long enough for the decoder to read it"
          unit="s"
          value={(settings.hold_ms ?? 5000) / 1000}
          min={0.5}
          max={120}
          step={0.5}
          onCommit={(seconds) => patch({ hold_ms: Math.round(seconds * 1000) })}
        />
      ) : (
        <FrequencyListChip
          label="Priority"
          title="Checked every 2 s and taken over anything else"
          values={settings.priority ?? []}
          plan={plan}
          addable
          onChange={(priority) => patch({ priority })}
        />
      )}
      {(settings.lockouts ?? []).length > 0 && (
        <FrequencyListChip
          label="Lockouts"
          title="Never held. Skip adds to this list."
          values={settings.lockouts ?? []}
          plan={plan}
          onChange={(lockouts) => patch({ lockouts })}
        />
      )}
      {firmwareSweep && (
        <ToggleChip
          label="Firmware sweep"
          title="Let the radio sweep itself"
          on={settings.hardware_sweep ?? true}
          onChange={(hardware_sweep) => patch({ hardware_sweep })}
        />
      )}
    </Chips>
  );
}

function BandChip({
  settings,
  plan,
  onSettings,
}: {
  settings: ScanSettings;
  plan: BandPlan | null;
  onSettings: (next: ScanSettings) => void;
}) {
  const presets = scanPresets(plan);
  const current = presetIn(settings, presets);
  return (
    <SettingChip
      label="Band"
      value={current?.name ?? "Custom"}
      title="Fill the scan from the band plan"
      disabled={presets.length === 0}
      padded={false}
      width="w-64"
    >
      {(close) => (
        <div
          role="listbox"
          aria-label="Band"
          className="flex max-h-72 flex-col overflow-y-auto py-1"
        >
          {presets.map((preset) => (
            <Button
              key={preset.id}
              type="button"
              role="option"
              aria-selected={preset.id === current?.id}
              className={listItem(preset.id === current?.id, false)}
              onClick={() => {
                onSettings(applyPreset(settings, preset));
                close();
              }}
            >
              {preset.name}
            </Button>
          ))}
        </div>
      )}
    </SettingChip>
  );
}

function FrequencyListChip({
  label,
  title,
  values,
  plan,
  addable = false,
  onChange,
}: {
  label: string;
  title: string;
  values: readonly number[];
  plan: BandPlan | null;
  addable?: boolean;
  onChange: (next: number[]) => void;
}) {
  return (
    <SettingChip
      label={label}
      value={values.length === 0 ? "none" : String(values.length)}
      quiet={values.length === 0}
      title={title}
    >
      {() => (
        <div className="flex flex-col gap-1">
          <ul className="flex max-h-48 flex-col overflow-y-auto font-mono text-xs">
            {values.map((hz) => (
              <li key={hz} className="flex items-center justify-between gap-2">
                <span>
                  {formatMhz(hz)} {channelName(plan, hz)}
                </span>
                <Button
                  type="button"
                  className={`${BTN_SM} hover:text-danger`}
                  aria-label={`Remove ${formatMhz(hz)}`}
                  onClick={() => onChange(withoutHz(values, hz))}
                >
                  <Icon glyph={X} size={12} />
                </Button>
              </li>
            ))}
          </ul>
          {addable && (
            <ChipField label="Add">
              <OptionalNumberField
                className="min-w-0 flex-1"
                label={`Add ${label.toLowerCase()} frequency`}
                placeholder="MHz"
                value={null}
                min={0}
                step={0.0125}
                unit="MHz"
                onCommit={(added) =>
                  added !== null && added > 0 && onChange(withHz(values, Math.round(added * 1e6)))
                }
              />
            </ChipField>
          )}
        </div>
      )}
    </SettingChip>
  );
}

function mhz(hz: number): number {
  return Number((hz / 1e6).toFixed(6));
}

function RangeChip({
  range,
  index,
  numbered,
  onPatch,
  onRemove,
}: {
  range: ScanRange;
  index: number;
  numbered: boolean;
  onPatch: (patch: Partial<ScanRange>) => void;
  onRemove: () => void;
}) {
  const name = `Range ${index + 1}`;
  const reversed = range.stop_hz < range.start_hz;
  return (
    <SettingChip
      label={numbered ? name : "Range"}
      value={`${mhz(range.start_hz)}-${mhz(range.stop_hz)}`}
      unit="MHz"
      title={`${name}, every ${range.step_hz / 1e3} kHz`}
      tone={reversed ? "danger" : undefined}
    >
      {(close) => (
        <div className="flex flex-col gap-2">
          <ChipField label="From">
            <NumberField
              className="min-w-0 flex-1"
              label={`${name} start`}
              value={mhz(range.start_hz)}
              min={0}
              step={0.1}
              onCommit={(start) => onPatch({ start_hz: Math.round(start * 1e6) })}
              unit="MHz"
            />
          </ChipField>
          <ChipField label="To">
            <NumberField
              className="min-w-0 flex-1"
              label={`${name} stop`}
              value={mhz(range.stop_hz)}
              min={0}
              step={0.1}
              invalid={reversed}
              onCommit={(stop) => onPatch({ stop_hz: Math.round(stop * 1e6) })}
              unit="MHz"
            />
          </ChipField>
          <ChipField label="Step">
            <NumberField
              className="min-w-0 flex-1"
              label={`${name} step`}
              value={range.step_hz / 1e3}
              min={MIN_STEP_KHZ}
              step={MIN_STEP_KHZ}
              onCommit={(step) => onPatch({ step_hz: Math.round(step * 1e3) })}
              unit="kHz"
            />
          </ChipField>
          <Button
            type="button"
            className={`${BTN_SM} self-start hover:text-danger`}
            aria-label={`Remove range ${index + 1}`}
            onClick={() => {
              onRemove();
              close();
            }}
          >
            <Icon glyph={X} size={12} />
            Remove
          </Button>
        </div>
      )}
    </SettingChip>
  );
}
