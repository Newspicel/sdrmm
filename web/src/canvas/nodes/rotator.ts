import type { Options } from "../../components/controls";
import { ROTATOR_LIMITS } from "../../lib/limits";
import type { RotatorAxes, RotatorLink, RotatorMode, RotatorNode, SkyPoint } from "../../lib/types";
import { compass } from "./satellite";

export const ROTATOR_BAUDS = [1_200, 4_800, 9_600, 19_200, 38_400, 57_600, 115_200].map((baud) => ({
  value: String(baud),
}));

export const ROTATOR_MODES: Options<RotatorMode> = [
  { value: "track", label: "Track", title: "Follow the wired satellite" },
  { value: "manual", label: "Manual", title: "Point where you set" },
  { value: "park", label: "Park", title: "Go to the park position" },
  { value: "stop", label: "Stop", title: "Halt both axes" },
];

export const ROTATOR_AXES: Options<RotatorAxes> = [
  { value: "azimuth_elevation", label: "Az + El" },
  { value: "azimuth", label: "Az only" },
];

export const AZIMUTH_RANGES: Options<number> = [360, 450, 540].map((deg) => ({
  value: deg,
  label: `${deg}°`,
}));

export function aimLabel(aim: SkyPoint | null | undefined, axes: RotatorAxes): string {
  if (aim == null) {
    return "-";
  }
  const bearing = `${aim.azimuth_deg.toFixed(0)}° ${compass(aim.azimuth_deg)}`;
  return axes === "azimuth" ? bearing : `${bearing}, ${aim.elevation_deg.toFixed(0)}°`;
}

export function elevationMax(axes: RotatorAxes, flip: boolean): number {
  if (axes === "azimuth") {
    return 0;
  }
  return flip ? ROTATOR_LIMITS.flip_elevation_deg : ROTATOR_LIMITS.level_elevation_deg;
}

export function flipsOver(link: RotatorLink | null | undefined): boolean {
  return link?.type !== "winegard";
}

export function flipped(data: RotatorNode): boolean {
  return (data.flip ?? false) && flipsOver(data.link);
}

export function withRange(data: RotatorNode, azimuthMaxDeg: number): Partial<RotatorNode> {
  const within = (aim: SkyPoint | undefined): SkyPoint | undefined =>
    aim === undefined
      ? undefined
      : { ...aim, azimuth_deg: Math.min(aim.azimuth_deg, azimuthMaxDeg) };
  return { azimuth_max_deg: azimuthMaxDeg, manual: within(data.manual), park: within(data.park) };
}

export function serialLink(device: string): RotatorLink {
  return { type: "gs232", device, baud: ROTATOR_LIMITS.gs232_baud };
}

export function winegardLink(device: string): RotatorLink {
  return { type: "winegard", device, baud: ROTATOR_LIMITS.winegard_baud };
}

export function rotctldLink(address: string = ROTATOR_LIMITS.rotctld_address): RotatorLink {
  return { type: "rotctld", address };
}

export function parsedBaud(value: string): number | null {
  const baud = Number(value);
  return Number.isInteger(baud) && baud >= 1_200 && baud <= 4_000_000 ? baud : null;
}
