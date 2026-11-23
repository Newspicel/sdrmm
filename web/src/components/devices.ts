import type {
  BackendSpec,
  DeviceBackend,
  DeviceClaim,
  DeviceInfo,
  DevicesResponse,
} from "../lib/types";
import type { Options } from "./controls";

/** Every backend the server drives, built-in or plugin, as `/api/devices` lists them. */
export type Backends = readonly BackendSpec[];

const GENERATOR_KEY_PREFIX = "gen-";

function reach(device: DeviceInfo, backends: Backends): string | undefined {
  return backends.find((entry) => entry.backend === device.driver)?.reach;
}

function nodeOwned(device: DeviceInfo, backends: Backends): boolean {
  return (
    reach(device, backends) === "playback" ||
    (isVirtualDevice(device, backends) && device.key.startsWith(GENERATOR_KEY_PREFIX))
  );
}

export function isVirtualDevice(device: DeviceInfo, backends: Backends): boolean {
  return reach(device, backends) === "synthetic";
}

export function rankDevices(
  devices: readonly DeviceInfo[],
  backends: Backends,
): readonly DeviceInfo[] {
  const rank = (device: DeviceInfo) => (isVirtualDevice(device, backends) ? 1 : 0);
  return devices.toSorted((a, b) => rank(a) - rank(b) || a.label.localeCompare(b.label));
}

export function visibleDevices(
  devices: readonly DeviceInfo[],
  showSynthetic: DevicesResponse["synthetic"],
  backends: Backends,
): readonly DeviceInfo[] {
  const pickable = devices.filter((device) => !nodeOwned(device, backends));
  return rankDevices(
    showSynthetic ? pickable : pickable.filter((device) => !isVirtualDevice(device, backends)),
    backends,
  );
}

export function unclaimedDevices(
  devices: readonly DeviceInfo[],
  claims: readonly DeviceClaim[],
  claimant: string,
): readonly DeviceInfo[] {
  return devices.filter(
    (device) =>
      !claims.some((claim) => claim.device === deviceId(device) && claim.node !== claimant),
  );
}

export type DeviceGroups = {
  radios: readonly DeviceInfo[];
  network: readonly DeviceInfo[];
  virtual: readonly DeviceInfo[];
};

export function groupDevices(
  devices: readonly DeviceInfo[],
  backends: Backends,
  adopted: readonly string[] = [],
): DeviceGroups {
  const added = (device: DeviceInfo) => adopted.includes(deviceId(device));
  return {
    radios: devices.filter((device) => !isVirtualDevice(device, backends) && !added(device)),
    network: devices.filter(added),
    virtual: devices.filter((device) => isVirtualDevice(device, backends)),
  };
}

export type SourceTab = "radios" | "network" | "recording" | "virtual";

function counted(label: string, count: number): string {
  return count > 0 ? `${label} (${count})` : label;
}

export function sourceTabs(groups: DeviceGroups): Options<SourceTab> {
  const tabs: { value: SourceTab; label: string; title: string }[] = [
    { value: "radios", label: "Radios", title: "Radios attached to this machine" },
    {
      value: "network",
      label: counted("Network", groups.network.length),
      title: "rtl_tcp, SpyServer, SDRconnect, KiwiSDR, AntSDR or Pluto",
    },
    { value: "recording", label: "Recording", title: "Play a recorded IQ file back" },
    {
      value: "virtual",
      label: counted("Virtual", groups.virtual.length),
      title: "Synthetic radios and the signal generator",
    },
  ];
  return tabs;
}

export function deviceId(device: DeviceInfo): string {
  return `${device.driver}:${device.key}`;
}

export type NetworkBackend = { driver: DeviceBackend; label: string; placeholder: string };

export function networkBackends(backends: Backends): readonly NetworkBackend[] {
  return backends
    .filter((entry) => entry.reach === "network")
    .map((entry) => ({
      driver: entry.backend,
      label: entry.label,
      placeholder: entry.address_hint ?? "",
    }));
}

export function networkDeviceId(driver: string, address: string): string | null {
  const scheme = /^([a-z][a-z0-9+._-]*):\/\//i.exec(address.trim());
  const trimmed = address
    .trim()
    .slice(scheme?.[0].length ?? 0)
    .replace(/\/+$/, "");
  if (trimmed === "" || /\s/.test(trimmed)) {
    return null;
  }
  const name = scheme?.[1]?.toLowerCase() ?? "";
  if (name === "https" || name === "wss") {
    return `${driver}:https://${trimmed}`;
  }
  const host = trimmed.slice(trimmed.lastIndexOf("@") + 1);
  const webDefault = (name === "http" || name === "ws") && !/:\d+$/.test(host);
  return `${driver}:${trimmed}${webDefault ? ":80" : ""}`;
}

export function networkRadioLabel(id: string, backends: Backends): string {
  const at = id.indexOf(":");
  const backend = networkBackends(backends).find((b) => b.driver === id.slice(0, at));
  const address = id.slice(at + 1).replace(/^https:\/\//, "");
  const host = address.slice(address.lastIndexOf("@") + 1);
  return backend === undefined ? host : `${backend.label} ${host}`;
}

export function deviceHint(device: DeviceInfo): string | undefined {
  return device.driver === "rtlsdr" && device.serial == null
    ? "Known by USB port, not serial: give it its own serial with rtl_eeprom -s"
    : undefined;
}
