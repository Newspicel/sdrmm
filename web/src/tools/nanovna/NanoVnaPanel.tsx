import { useQuery } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { ALERT } from "../../components/controls";
import { Segmented } from "../../components/Segmented";
import { bandPlanQuery, bandRegionsQuery, toolRunQuery } from "../../lib/api";
import type { NanoVnaCalibration, NanoVnaDeviceReport, NanoVnaSweep } from "../../lib/types";
import type { Recording } from "./AntennaLibrary";
import { hamBands } from "./bands";
import { CalibrationPanel } from "./CalibrationPanel";
import { DeviceReport } from "./DeviceReport";
import { type ExportSource, savedSource, sweepSource } from "./export";
import { InstrumentBar } from "./Instrument";
import { MeasureView, type Primary } from "./MeasureView";
import {
  nanoVnaDescribeRequest,
  nanoVnaDevices,
  nanoVnaDevicesRequest,
  nanoVnaReport,
} from "./nanovna";
import type { ReferenceSource } from "./references";
import { type Range, SweepBar } from "./SweepBar";
import { useSweeper } from "./useSweeper";

type Tab = "measure" | "calibrate" | "device";

const TABS = [
  { value: "measure", label: "Measure" },
  { value: "calibrate", label: "Calibrate" },
  { value: "device", label: "Device" },
] as const;

const DEFAULT_RANGE: Range = { startHz: 1_000_000, stopHz: 30_000_000, points: 401, averages: 1 };

const LIVE_KEY = "live";

interface Stamped {
  report: NanoVnaDeviceReport | null;
  at: number;
}

export function NanoVnaPanel() {
  const [chosenPort, setChosenPort] = useState("");
  const [range, setRange] = useState<Range>(DEFAULT_RANGE);
  const [tab, setTab] = useState<Tab>("measure");
  const [opened, setOpened] = useState<{ source: ReferenceSource; after: number } | null>(null);
  const [edited, setEdited] = useState<Stamped | null>(null);

  const devicesQuery = useQuery(toolRunQuery(nanoVnaDevicesRequest()));
  const devices = nanoVnaDevices(devicesQuery.data);
  const port = chosenPort || devices[0]?.port || "";
  const describeQuery = useQuery(toolRunQuery(port === "" ? null : nanoVnaDescribeRequest(port)));
  const bands = useHamBands();

  const request =
    port === ""
      ? null
      : {
          port,
          start_hz: range.startHz,
          stop_hz: range.stopHz,
          points: range.points,
          averages: range.averages,
        };
  const sweeper = useSweeper(request);
  const report = newest([
    { report: nanoVnaReport(describeQuery.data), at: describeQuery.dataUpdatedAt },
    { report: sweeper.sweep?.device ?? null, at: sweeper.at },
    edited,
  ]);
  const editReport = (next: NanoVnaDeviceReport) => setEdited({ report: next, at: Date.now() });
  const updateCalibration = (calibration: NanoVnaCalibration) => {
    if (report !== null) {
      editReport({ ...report, calibration });
    }
  };
  const showOpened = opened !== null && opened.after === sweeper.count;
  const primary: Primary | null = showOpened
    ? { key: opened.source.key, label: opened.source.label, points: opened.source.points }
    : sweeper.sweep === null
      ? null
      : { key: LIVE_KEY, label: "Live", points: sweeper.sweep.points };

  return (
    <div className="flex min-h-full flex-col gap-3">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <InstrumentBar
          devices={devices}
          port={port}
          scanning={devicesQuery.isFetching}
          report={report}
          onPort={setChosenPort}
          onRescan={() => void devicesQuery.refetch()}
        />
        <Segmented label="NanoVNA section" value={tab} options={TABS} onChange={setTab} />
      </div>
      {devicesQuery.isError && <p className={ALERT}>{devicesQuery.error.message}</p>}

      {tab === "measure" && (
        <>
          <SweepBar
            range={range}
            bands={bands}
            maxPoints={report?.max_points ?? null}
            busy={sweeper.busy}
            live={sweeper.live}
            canSweep={request !== null}
            status={statusOf(sweeper.sweep, sweeper.live, showOpened)}
            exportable={exportable(showOpened ? opened.source : null, sweeper.sweep)}
            onRange={setRange}
            onSweep={() => request !== null && sweeper.once(request)}
            onLive={sweeper.setLive}
          />
          {sweeper.error !== null && <p className={ALERT}>{sweeper.error}</p>}
          <MeasureView
            primary={primary}
            recording={showOpened ? null : recordingOf(sweeper.sweep)}
            bands={bands}
            onOpen={(source) => setOpened({ source, after: sweeper.count })}
            onHarmonicGrid={() =>
              setRange({
                ...range,
                startHz: Math.max(10_000, Math.round(range.stopHz / range.points)),
              })
            }
          />
        </>
      )}

      {tab === "calibrate" && (
        <CalibrationPanel
          port={port}
          range={{
            start_hz: range.startHz,
            stop_hz: range.stopHz,
            points: Math.min(range.points, report?.max_points ?? 101),
          }}
          state={report?.calibration ?? null}
          onState={updateCalibration}
        />
      )}

      {tab === "device" &&
        (report === null ? (
          <p className="text-xs text-ink-faint">
            {describeQuery.isFetching ? "Reading the instrument…" : "No instrument selected."}
          </p>
        ) : (
          <DeviceReport
            report={report}
            reading={describeQuery.isFetching}
            onReread={() => void describeQuery.refetch()}
            onReport={editReport}
          />
        ))}
      {describeQuery.isError && tab !== "measure" && (
        <p className={ALERT}>{describeQuery.error.message}</p>
      )}
    </div>
  );
}

function useHamBands() {
  const regions = useQuery(bandRegionsQuery());
  const plan = useQuery(bandPlanQuery(regions.data?.default_region ?? null));
  return useMemo(() => (plan.data === undefined ? [] : hamBands(plan.data)), [plan.data]);
}

function newest(candidates: readonly (Stamped | null)[]): NanoVnaDeviceReport | null {
  let best: Stamped | null = null;
  for (const candidate of candidates) {
    if (candidate?.report != null && (best === null || candidate.at >= best.at)) {
      best = candidate;
    }
  }
  return best?.report ?? null;
}

function statusOf(sweep: NanoVnaSweep | null, live: boolean, opened: boolean): string | null {
  if (sweep === null || opened) {
    return null;
  }
  return `${sweep.points.length} points in ${(sweep.elapsed_ms / 1000).toFixed(1)} s${live ? ", live" : ""}`;
}

function recordingOf(sweep: NanoVnaSweep | null): Recording | null {
  if (sweep === null) {
    return null;
  }
  const device = sweep.device;
  return {
    points: sweep.points,
    instrument: `${device.board ?? "NanoVNA"} ${device.firmware}`,
    label: new Date().toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    }),
  };
}

function exportable(
  opened: ReferenceSource | null,
  sweep: NanoVnaSweep | null,
): ExportSource | null {
  if (opened !== null) {
    return savedSource(opened.label, opened.points, null);
  }
  return sweep === null ? null : sweepSource(sweep);
}
