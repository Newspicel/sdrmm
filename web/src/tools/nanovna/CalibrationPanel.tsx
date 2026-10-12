import { useMutation } from "@tanstack/react-query";
import { Check } from "lucide-react";
import { useState } from "react";
import { Button } from "../../components/BaseControls";
import { ALERT, BTN, BTN_DANGER, BTN_PRIMARY, LABEL } from "../../components/controls";
import { formatHz } from "../../components/format";
import { Icon } from "../../components/Icon";
import { Select } from "../../components/Select";
import { runTool } from "../../lib/api";
import type { NanoVnaCalibration, NanoVnaStandard, NanoVnaSweepState } from "../../lib/types";
import { measuredStandards } from "./calibration";
import { type CalibrationStep, nanoVnaCalibrateRequest, nanoVnaCalibration } from "./nanovna";

interface Standard {
  standard: NanoVnaStandard;
  label: string;
  hint: string;
  optional: boolean;
}

const STANDARDS: readonly Standard[] = [
  { standard: "open", label: "Open", hint: "Open standard on CH0, or nothing", optional: false },
  { standard: "short", label: "Short", hint: "Short standard on CH0", optional: false },
  { standard: "load", label: "Load", hint: "50 Ω load on CH0", optional: false },
  { standard: "isolation", label: "Isolation", hint: "Loads on CH0 and CH1", optional: true },
  { standard: "thru", label: "Thru", hint: "CH0 joined to CH1", optional: true },
];

const SLOT_OPTIONS = [0, 1, 2, 3, 4, 5, 6].map((slot) => ({ value: slot, label: `Slot ${slot}` }));

export function CalibrationPanel({
  port,
  range,
  state,
  onState,
}: {
  port: string;
  range: NanoVnaSweepState;
  state: NanoVnaCalibration | null;
  onState: (state: NanoVnaCalibration) => void;
}) {
  const [slot, setSlot] = useState(0);
  const [pending, setPending] = useState<string | null>(null);
  const calibrate = useMutation({
    mutationFn: async ({ step }: { step: CalibrationStep; key: string }) =>
      nanoVnaCalibration(
        await runTool(
          nanoVnaCalibrateRequest(port, step, step.step === "reset" ? range : undefined),
        ),
      ),
    onSettled: () => setPending(null),
    onSuccess: (next) => {
      if (next !== null) {
        onState(next);
      }
    },
  });

  function run(key: string, step: CalibrationStep) {
    setPending(key);
    calibrate.mutate({ step, key });
  }

  const measured = state === null ? new Set<NanoVnaStandard>() : measuredStandards(state);
  const busy = calibrate.isPending || port === "";
  const ready = STANDARDS.every((entry) => entry.optional || measured.has(entry.standard));

  return (
    <div className="flex max-w-3xl flex-col gap-4">
      <span className="font-mono text-xs text-ink-dim" title="Set on the Measure tab">
        {formatHz(range.start_hz)} to {formatHz(range.stop_hz)}, {range.points} points
      </span>

      <ol className="flex flex-col">
        <Step number={1} label="Start" hint="Clears the old calibration and sets the range above">
          <Button
            type="button"
            className={BTN}
            disabled={busy}
            onClick={() => run("reset", { step: "reset" })}
          >
            {pending === "reset" ? "Starting…" : "Start over"}
          </Button>
        </Step>
        {STANDARDS.map((entry, index) => (
          <Step
            key={entry.standard}
            number={index + 2}
            label={entry.optional ? `${entry.label}, optional` : entry.label}
            hint={entry.hint}
            done={measured.has(entry.standard)}
          >
            <Button
              type="button"
              className={BTN}
              disabled={busy}
              onClick={() => run(entry.standard, { step: entry.standard })}
            >
              {pending === entry.standard ? "Measuring…" : "Measure"}
            </Button>
          </Step>
        ))}
        <Step
          number={STANDARDS.length + 2}
          label="Apply"
          hint="Computes the correction and turns it on"
          done={state?.applied === true && ready}
        >
          <Button
            type="button"
            className={BTN_PRIMARY}
            disabled={busy || !ready}
            title={ready ? undefined : "Measure open, short and load first"}
            onClick={() => run("finish", { step: "finish" })}
          >
            {pending === "finish" ? "Applying…" : "Apply"}
          </Button>
        </Step>
      </ol>

      <div className="flex flex-wrap items-end gap-2 border-t border-line pt-4">
        <div className="flex flex-col gap-1">
          <span className={LABEL}>Stored on the device</span>
          <Select
            label="Calibration slot"
            value={slot}
            options={SLOT_OPTIONS}
            onChange={setSlot}
            className="w-28"
          />
        </div>
        <Button
          type="button"
          className={BTN}
          disabled={busy}
          onClick={() => run("save", { step: "save", slot })}
        >
          {pending === "save" ? "Saving…" : "Save"}
        </Button>
        <Button
          type="button"
          className={BTN}
          disabled={busy}
          onClick={() => run("recall", { step: "recall", slot })}
        >
          {pending === "recall" ? "Loading…" : "Load"}
        </Button>
        <Button
          type="button"
          className={BTN}
          disabled={busy}
          onClick={() => run("toggle", { step: state?.applied === true ? "disable" : "enable" })}
        >
          {state?.applied === true ? "Turn correction off" : "Turn correction on"}
        </Button>
        <Button
          type="button"
          className={BTN_DANGER}
          disabled={busy}
          onClick={() => run("reset", { step: "reset" })}
        >
          Clear
        </Button>
      </div>

      {calibrate.isError && <p className={ALERT}>{calibrate.error.message}</p>}
    </div>
  );
}

function Step({
  number,
  label,
  hint,
  done = false,
  children,
}: {
  number: number;
  label: string;
  hint: string;
  done?: boolean;
  children: React.ReactNode;
}) {
  return (
    <li className="flex items-center gap-3 border-b border-line/60 py-2">
      <span
        className={`flex size-6 shrink-0 items-center justify-center rounded-full border font-mono text-2xs ${
          done ? "border-ok bg-ok/15 text-ok" : "border-line-strong text-ink-dim"
        }`}
        aria-label={done ? `${label} done` : undefined}
      >
        {done ? <Icon glyph={Check} size={12} /> : number}
      </span>
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="text-sm text-ink">{label}</span>
        <span className="truncate text-xs text-ink-faint">{hint}</span>
      </div>
      {children}
    </li>
  );
}
