import { useQuery } from "@tanstack/react-query";
import { serialDetail } from "../canvas/nodes/gpsSource";
import { serialPortsQuery } from "../lib/api";
import type { Options } from "./controls";
import { ChoiceChip } from "./face/Chips";

const NONE = "";

function portName(path: string): string {
  return path.split("/").pop() ?? path;
}

/** A serial port of the server's machine, stored as its path. None picked is `null`. */
export function SerialPortChip({
  label,
  title,
  quiet,
  empty,
  value,
  onCommit,
}: {
  label: string;
  title: string;
  quiet: boolean;
  empty: string;
  value: unknown;
  onCommit: (path: string | null) => void;
}) {
  const devices = useQuery(serialPortsQuery()).data?.devices ?? [];
  const chosen = typeof value === "string" ? value : NONE;
  const missing = chosen !== NONE && !devices.some((device) => device.path === chosen);
  const options: Options<string> = [
    { value: NONE, label: empty },
    ...devices.map((device) => ({
      value: device.path,
      label: portName(device.path),
      title: serialDetail(device) || device.path,
    })),
    ...(missing
      ? [{ value: chosen, label: portName(chosen), title: `${chosen}, not plugged in` }]
      : []),
  ];
  return (
    <ChoiceChip
      label={label}
      title={title}
      quiet={quiet}
      value={chosen}
      options={options}
      onChange={(path) => onCommit(path === NONE ? null : path)}
    />
  );
}
