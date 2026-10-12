import { Collapsible } from "@base-ui/react/collapsible";
import { useMutation } from "@tanstack/react-query";
import { Button } from "../../components/BaseControls";
import { ALERT, BTN, LABEL } from "../../components/controls";
import { formatHz } from "../../components/format";
import { Select } from "../../components/Select";
import { runTool } from "../../lib/api";
import type { NanoVnaDeviceReport, NanoVnaPower } from "../../lib/types";
import { formatSi, nanoVnaConfigureRequest, nanoVnaReport } from "./nanovna";

const BANDWIDTHS = [10, 30, 100, 333, 1000, 2000, 4000];

const POWERS: ReadonlyArray<{ value: string; label: string; power: NanoVnaPower }> = [
  { value: "auto", label: "Auto", power: "auto" },
  ...[0, 1, 2, 3].map((level) => ({
    value: String(level),
    label: `${2 * (level + 1)} mA`,
    power: { level },
  })),
];

const AUTO_POWER = 255;

export function DeviceReport({
  report,
  reading,
  onReread,
  onReport,
}: {
  report: NanoVnaDeviceReport;
  reading: boolean;
  onReread: () => void;
  onReport: (report: NanoVnaDeviceReport) => void;
}) {
  const configure = useMutation({
    mutationFn: async (change: { bandwidth_hz?: number; power?: NanoVnaPower }) =>
      nanoVnaReport(await runTool(nanoVnaConfigureRequest({ port: report.port, ...change }))),
    onSuccess: (next) => {
      if (next !== null) {
        onReport(next);
      }
    },
  });
  const sweep = report.sweep;
  const bandwidth = report.bandwidth_hz ?? 1000;
  const power = report.power == null || report.power === AUTO_POWER ? "auto" : String(report.power);

  return (
    <div className="flex max-w-4xl flex-col gap-5">
      <section className="flex flex-wrap items-end gap-4">
        <Setting label="IF bandwidth" hint="Narrower is quieter and slower">
          <Select
            label="IF bandwidth"
            value={bandwidth}
            options={[...new Set([...BANDWIDTHS, bandwidth])]
              .toSorted((left, right) => left - right)
              .map((hz) => ({ value: hz, label: formatHz(hz) }))}
            onChange={(hz) => configure.mutate({ bandwidth_hz: hz })}
            disabled={configure.isPending}
            className="w-32"
          />
        </Setting>
        <Setting label="Drive" hint="Output drive current; auto picks it per frequency">
          <Select
            label="Drive"
            value={power}
            options={POWERS}
            onChange={(value) => {
              const picked = POWERS.find((entry) => entry.value === value);
              if (picked !== undefined) {
                configure.mutate({ power: picked.power });
              }
            }}
            disabled={configure.isPending}
            className="w-28"
          />
        </Setting>
        <Button type="button" className={BTN} disabled={reading} onClick={onReread}>
          {reading ? "Reading…" : "Read again"}
        </Button>
      </section>
      {configure.isError && <p className={ALERT}>{configure.error.message}</p>}

      <div className="grid gap-x-8 gap-y-4 sm:grid-cols-2 lg:grid-cols-3">
        <Group title="Instrument">
          <Entry label="Board" value={report.board ?? "unnamed"} />
          <Entry label="Firmware" value={report.firmware} />
          <Entry label="Port" value={report.port} />
          <Entry
            label="Battery"
            value={report.battery_mv == null ? "-" : `${(report.battery_mv / 1000).toFixed(3)} V`}
          />
        </Group>
        <Group title="Measurement">
          <Entry
            label="Points per scan"
            value={report.max_points == null ? "-" : String(report.max_points)}
          />
          <Entry
            label="Electrical delay"
            value={
              report.electrical_delay_s == null ? "-" : formatSi(report.electrical_delay_s, "s", 3)
            }
          />
          <Entry
            label="S21 offset"
            value={report.s21_offset_db == null ? "-" : `${report.s21_offset_db.toFixed(3)} dB`}
          />
          <Entry
            label="Calibration"
            value={report.calibration.raw === "" ? "none" : report.calibration.raw}
          />
        </Group>
        <Group title="Reference">
          <Entry label="TCXO" value={report.tcxo_hz == null ? "-" : formatHz(report.tcxo_hz)} />
          <Entry
            label="Harmonics above"
            value={
              report.harmonic_threshold_hz == null ? "-" : formatHz(report.harmonic_threshold_hz)
            }
          />
          <Entry
            label="Screen sweep"
            value={
              sweep == null ? "-" : `${formatHz(sweep.start_hz)} to ${formatHz(sweep.stop_hz)}`
            }
          />
        </Group>
      </div>

      {report.info.length > 0 && (
        <Collapsible.Root className="text-xs text-ink-dim">
          <Collapsible.Trigger className="cursor-pointer text-xs text-ink-dim hover:text-ink">
            Device info
          </Collapsible.Trigger>
          <Collapsible.Panel>
            <pre className="mt-2 overflow-x-auto rounded-[3px] border border-line bg-panel-2 p-2 font-mono text-[11px] leading-relaxed">
              {report.info.join("\n")}
            </pre>
          </Collapsible.Panel>
        </Collapsible.Root>
      )}
    </div>
  );
}

function Setting({
  label,
  hint,
  children,
}: {
  label: string;
  hint: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1">
      <span className={LABEL} title={hint}>
        {label}
      </span>
      {children}
    </div>
  );
}

function Group({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="flex min-w-0 flex-col gap-1">
      <h4 className={LABEL}>{title}</h4>
      <dl className="flex flex-col gap-0.5">{children}</dl>
    </section>
  );
}

function Entry({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-baseline justify-between gap-3 border-b border-line/40 py-0.5">
      <dt className="shrink-0 text-xs text-ink-dim">{label}</dt>
      <dd className="truncate font-mono text-xs text-ink tabular-nums" title={value}>
        {value}
      </dd>
    </div>
  );
}
