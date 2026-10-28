import backends from "../generated/backends.json" with { type: "json" };
import type { DeviceBackend, DeviceClaim, DeviceInfo, DevicesResponse } from "../lib/types";
import type { Options } from "./controls";

export type BackendEntry = (typeof backends)[number];

const GENERATOR_KEY_PREFIX = "gen-";

const BY_ID = new Map<string, BackendEntry>(backends.map((entry) => [entry.backend, entry]));

export function backendEntry(driver: string): BackendEntry | undefined {
  return BY_ID.get(driver);
}

function reach(device: DeviceInfo): string | undefined {
  return BY_ID.get(device.driver)?.reach;
}

function nodeOwned(device: DeviceInfo): boolean {
  return (
    reach(device) === "playback" ||
    (isVirtualDevice(device) && device.key.startsWith(GENERATOR_KEY_PREFIX))
  );
}

function deviceRank(device: DeviceInfo): number {
  return isVirtualDevice(device) ? 1 : 0;
}

export function isVirtualDevice(device: DeviceInfo): boolean {
  return reach(device) === "synthetic";
}

export function rankDevices(devices: readonly DeviceInfo[]): readonly DeviceInfo[] {
  return devices.toSorted(
    (a, b) => deviceRank(a) - deviceRank(b) || a.label.localeCompare(b.label),
  );
}

export function visibleDevices(
  devices: readonly DeviceInfo[],
  showSynthetic: DevicesResponse["synthetic"],
): readonly DeviceInfo[] {
  const pickable = devices.filter((device) => !nodeOwned(device));
  return rankDevices(
    showSynthetic ? pickable : pickable.filter((device) => !isVirtualDevice(device)),
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

export function groupDevices(devices: readonly DeviceInfo[]): {
  radios: readonly DeviceInfo[];
  virtual: readonly DeviceInfo[];
} {
  return {
    radios: devices.filter((device) => !isVirtualDevice(device)),
    virtual: devices.filter(isVirtualDevice),
  };
}

export type SourceTab = "radios" | "network" | "recording" | "virtual";

function counted(label: string, count: number): string {
  return count > 0 ? `${label} (${count})` : label;
}

export function sourceTabs(groups: {
  radios: readonly DeviceInfo[];
  virtual: readonly DeviceInfo[];
}): Options<SourceTab> {
  const tabs: { value: SourceTab; label: string; title: string }[] = [
    { value: "radios", label: "Radios", title: "Radios attached to this machine" },
    {
      value: "network",
      label: "Network",
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

export const NETWORK_BACKENDS: readonly [NetworkBackend, ...NetworkBackend[]] = backends
  .filter((entry) => entry.reach === "network")
  .map((entry) => ({
    driver: entry.backend as DeviceBackend,
    label: entry.label,
    placeholder: entry.address_hint ?? "",
  })) as unknown as readonly [NetworkBackend, ...NetworkBackend[]];

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

export function networkRadioLabel(id: string): string {
  const at = id.indexOf(":");
  const backend = NETWORK_BACKENDS.find((b) => b.driver === id.slice(0, at));
  const address = id.slice(at + 1).replace(/^https:\/\//, "");
  const host = address.slice(address.lastIndexOf("@") + 1);
  return backend === undefined ? host : `${backend.label} ${host}`;
}

export function deviceHint(device: DeviceInfo): string | undefined {
  return device.driver === "rtlsdr" && device.serial == null
    ? "Known by USB port, not serial: give it its own serial with rtl_eeprom -s"
    : undefined;
}
