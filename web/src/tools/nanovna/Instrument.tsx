import {
  BatteryFull,
  BatteryLow,
  BatteryMedium,
  CircleAlert,
  CircleCheck,
  RefreshCw,
} from "lucide-react";
import { useState } from "react";
import { Button, Form, Input } from "../../components/BaseControls";
import { FIELD, ICON_BTN } from "../../components/controls";
import { Icon } from "../../components/Icon";
import { Select } from "../../components/Select";
import type { NanoVnaDevice, NanoVnaDeviceReport } from "../../lib/types";
import { calibrationState } from "./calibration";

const BATTERY_FULL_MV = 4000;
const BATTERY_LOW_MV = 3500;

export function InstrumentBar({
  devices,
  port,
  scanning,
  report,
  onPort,
  onRescan,
}: {
  devices: readonly NanoVnaDevice[];
  port: string;
  scanning: boolean;
  report: NanoVnaDeviceReport | null;
  onPort: (port: string) => void;
  onRescan: () => void;
}) {
  return (
    <div className="flex min-w-0 flex-wrap items-center gap-x-4 gap-y-2">
      <div className="flex items-center gap-1">
        <PortPicker devices={devices} port={port} onPort={onPort} />
        <Button
          type="button"
          className={ICON_BTN}
          aria-label="Look for instruments again"
          title="Look for instruments again"
          disabled={scanning}
          onClick={onRescan}
        >
          <span className={scanning ? "animate-spin" : ""}>
            <Icon glyph={RefreshCw} size={14} />
          </span>
        </Button>
      </div>
      {report !== null && <Status report={report} />}
    </div>
  );
}

function PortPicker({
  devices,
  port,
  onPort,
}: {
  devices: readonly NanoVnaDevice[];
  port: string;
  onPort: (port: string) => void;
}) {
  const [typed, setTyped] = useState("");
  if (devices.length === 0) {
    return (
      <Form
        onSubmit={(event) => {
          event.preventDefault();
          onPort(typed.trim());
        }}
      >
        <Input
          aria-label="Serial port"
          data-hotkeys="off"
          value={typed}
          placeholder="No NanoVNA found, type its port"
          onChange={(event) => setTyped(event.target.value)}
          onBlur={() => onPort(typed.trim())}
          className={`${FIELD} w-64`}
        />
      </Form>
    );
  }
  return (
    <Select
      label="Instrument"
      value={port}
      options={devices.map((device) => ({
        value: device.port,
        label: `${device.model ?? "NanoVNA"} · ${device.port.replace(/^\/dev\//, "")}`,
      }))}
      onChange={onPort}
      className="w-72"
    />
  );
}

function Status({ report }: { report: NanoVnaDeviceReport }) {
  const calibration = calibrationState(report.calibration);
  return (
    <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs">
      <span className="text-ink-dim" title={report.info.join("\n")}>
        {report.board ?? "NanoVNA"}{" "}
        <span className="font-mono text-ink-faint">{report.firmware}</span>
      </span>
      {report.battery_mv != null && <Battery millivolts={report.battery_mv} />}
      <span
        className={`inline-flex items-center gap-1 font-medium ${calibration.tone}`}
        title={calibration.detail}
      >
        <Icon glyph={calibration.ok ? CircleCheck : CircleAlert} size={12} />
        {calibration.label}
      </span>
    </div>
  );
}

function Battery({ millivolts }: { millivolts: number }) {
  const glyph =
    millivolts >= BATTERY_FULL_MV
      ? BatteryFull
      : millivolts >= BATTERY_LOW_MV
        ? BatteryMedium
        : BatteryLow;
  return (
    <span
      className={`inline-flex items-center gap-1 font-mono ${
        millivolts < BATTERY_LOW_MV ? "text-warn" : "text-ink-dim"
      }`}
      title="Battery"
    >
      <Icon glyph={glyph} size={14} />
      {(millivolts / 1000).toFixed(2)} V
    </span>
  );
}
