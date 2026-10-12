import { Download, Play, Square } from "lucide-react";
import { useState } from "react";
import { Button } from "../../components/BaseControls";
import { BTN, BTN_PRIMARY, LABEL } from "../../components/controls";
import { Icon } from "../../components/Icon";
import { NumberField } from "../../components/NumberField";
import { Popover } from "../../components/Popover";
import { Select } from "../../components/Select";
import type { HamBand } from "./bands";
import {
  downloadText,
  type ExportSource,
  exportFilename,
  sweepCsv,
  TOUCHSTONE_FORMATS,
  type TouchstoneFormat,
  touchstoneS1p,
  touchstoneS2p,
} from "./export";

export interface Range {
  startHz: number;
  stopHz: number;
  points: number;
  averages: number;
}

const POINT_OPTIONS = [51, 101, 201, 401, 801, 1601, 3201].map((points) => ({
  value: points,
  label: String(points),
}));

const MIN_HZ = 10_000;
const MAX_HZ = 6_300_000_000;

const WIDE = [
  { key: "hf", label: "HF 1 to 30 MHz", startHz: 1_000_000, stopHz: 30_000_000 },
  { key: "vhf", label: "VHF 30 to 300 MHz", startHz: 30_000_000, stopHz: 300_000_000 },
  { key: "full", label: "50 kHz to 900 MHz", startHz: 50_000, stopHz: 900_000_000 },
] as const;

export function bandRange(band: HamBand): { startHz: number; stopHz: number } {
  const margin = Math.max((band.stopHz - band.startHz) * 0.5, band.startHz * 0.01);
  return {
    startHz: Math.max(MIN_HZ, Math.round(band.startHz - margin)),
    stopHz: Math.min(MAX_HZ, Math.round(band.stopHz + margin)),
  };
}

export function SweepBar({
  range,
  bands,
  maxPoints,
  busy,
  live,
  canSweep,
  status,
  exportable,
  onRange,
  onSweep,
  onLive,
}: {
  range: Range;
  bands: readonly HamBand[];
  maxPoints: number | null;
  busy: boolean;
  live: boolean;
  canSweep: boolean;
  status: string | null;
  exportable: ExportSource | null;
  onRange: (range: Range) => void;
  onSweep: () => void;
  onLive: (live: boolean) => void;
}) {
  const presets = presetsFor(bands);
  const preset = presets.find(
    (entry) => entry.startHz === range.startHz && entry.stopHz === range.stopHz,
  );
  const invalid = range.stopHz <= range.startHz;

  return (
    <div className="flex flex-wrap items-end gap-x-3 gap-y-2">
      <Field label="Band">
        <Select
          label="Band"
          value={preset?.key ?? ""}
          options={[
            { value: "", label: "Custom" },
            ...presets.map(({ key, label }) => ({ value: key, label })),
          ]}
          onChange={(key) => {
            const picked = presets.find((entry) => entry.key === key);
            if (picked !== undefined) {
              onRange({ ...range, startHz: picked.startHz, stopHz: picked.stopHz });
            }
          }}
          className="w-44"
        />
      </Field>
      <Field label="Start">
        <NumberField
          label="Start frequency"
          unit="MHz"
          value={range.startHz / 1e6}
          onCommit={(mhz) => onRange({ ...range, startHz: Math.round(mhz * 1e6) })}
          min={MIN_HZ / 1e6}
          max={MAX_HZ / 1e6}
          step={0.001}
          invalid={invalid}
          className="w-32"
        />
      </Field>
      <Field label="Stop">
        <NumberField
          label="Stop frequency"
          unit="MHz"
          value={range.stopHz / 1e6}
          onCommit={(mhz) => onRange({ ...range, stopHz: Math.round(mhz * 1e6) })}
          min={MIN_HZ / 1e6}
          max={MAX_HZ / 1e6}
          step={0.001}
          invalid={invalid}
          className="w-32"
        />
      </Field>
      <Field label="Points">
        <Select
          label="Points"
          value={range.points}
          options={POINT_OPTIONS.map((option) => ({
            ...option,
            label:
              maxPoints !== null && option.value > maxPoints
                ? `${option.label}, in parts`
                : option.label,
          }))}
          onChange={(points) => onRange({ ...range, points })}
          className="w-36"
        />
      </Field>
      <Field label="Average">
        <NumberField
          label="Sweeps to average"
          value={range.averages}
          onCommit={(averages) => onRange({ ...range, averages: Math.round(averages) })}
          min={1}
          max={16}
          step={1}
          className="w-20"
        />
      </Field>
      <div className="flex items-center gap-1.5">
        <Button
          type="button"
          className={`${BTN_PRIMARY} min-w-24 justify-center`}
          disabled={!canSweep || invalid || busy || live}
          onClick={onSweep}
        >
          {busy && !live ? "Sweeping…" : "Sweep"}
        </Button>
        <Button
          type="button"
          aria-pressed={live}
          className={`${BTN} ${live ? "border-danger/70 text-danger" : ""}`}
          disabled={!canSweep || invalid}
          title="Sweep again as soon as each sweep ends"
          onClick={() => onLive(!live)}
        >
          <Icon glyph={live ? Square : Play} size={12} filled />
          {live ? "Stop" : "Live"}
        </Button>
        {exportable !== null && <ExportMenu source={exportable} />}
      </div>
      {status !== null && (
        <span className="pb-1.5 font-mono text-2xs text-ink-faint tabular-nums">{status}</span>
      )}
    </div>
  );
}

function presetsFor(bands: readonly HamBand[]) {
  return [
    ...WIDE,
    ...bands
      .filter((band) => band.stopHz <= MAX_HZ && band.startHz >= MIN_HZ)
      .map((band) => ({ key: `band:${band.label}`, label: band.label, ...bandRange(band) })),
  ];
}

function ExportMenu({ source }: { source: ExportSource }) {
  const [format, setFormat] = useState<TouchstoneFormat>("ri");
  const files = [
    {
      label: "Touchstone .s1p",
      ext: "s1p",
      mime: "application/octet-stream",
      text: (recordedAt: string) => touchstoneS1p(source, format, { recordedAt }),
    },
    {
      label: "Touchstone .s2p",
      ext: "s2p",
      mime: "application/octet-stream",
      text: (recordedAt: string) => touchstoneS2p(source, format, { recordedAt }),
    },
    { label: "Table .csv", ext: "csv", mime: "text/csv", text: () => sweepCsv(source) },
  ];
  return (
    <Popover
      label={
        <>
          <Icon glyph={Download} size={12} />
          Export
        </>
      }
      triggerClass={BTN}
      width="w-60"
      align="end"
    >
      {(close) => (
        <div className="flex flex-col gap-2">
          <Field label="Numbers">
            <Select
              label="Touchstone number format"
              value={format}
              options={TOUCHSTONE_FORMATS}
              onChange={setFormat}
              className="w-full"
            />
          </Field>
          {files.map((file) => (
            <Button
              key={file.ext}
              type="button"
              className={`${BTN} justify-start`}
              onClick={() => {
                downloadText(
                  exportFilename(source, file.ext),
                  file.mime,
                  file.text(new Date().toISOString()),
                );
                close();
              }}
            >
              {file.label}
            </Button>
          ))}
        </div>
      )}
    </Popover>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-1">
      <span className={LABEL}>{label}</span>
      {children}
    </div>
  );
}
