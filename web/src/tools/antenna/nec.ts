import type {
  AntennaGround,
  AntennaMaterial,
  AntennaPoint,
  AntennaReport,
  AntennaSimulation,
} from "../../lib/types";
import { designLabel, type ModelSettings } from "./antenna";

const CONDUCTIVITY: Record<AntennaMaterial, number | null> = {
  lossless: null,
  copper: 5.8e7,
  aluminium: 3.5e7,
  stainless_steel: 1.4e6,
};

const SOIL: Record<Exclude<AntennaGround, "free_space" | "perfect">, [number, number]> = {
  poor: [5, 0.001],
  average: [13, 0.005],
  good: [20, 0.03],
  seawater: [81, 5],
};

export function necDeck(
  report: AntennaReport,
  simulation: AntennaSimulation,
  model: ModelSettings,
): string {
  const grounded = model.ground !== "free_space";
  const lift = grounded ? model.height_m - report.geometry.feed.y_m : 0;
  const sweep = simulation.sweep;
  const first = sweep[0]?.frequency_hz ?? report.frequency_hz;
  const last = sweep[sweep.length - 1]?.frequency_hz ?? report.frequency_hz;
  const step = sweep.length > 1 ? (last - first) / (sweep.length - 1) : 0;
  const conductivity = CONDUCTIVITY[model.material];
  return [
    `CM ${designLabel(report.design.type)} for ${(report.frequency_hz / 1e6).toFixed(4)} MHz`,
    `CM Wire ${model.wire_diameter_mm} mm, ${model.material.replace("_", " ")}`,
    grounded ? `CM Feedpoint ${model.height_m} m above ${model.ground} ground` : "CM Free space",
    "CE",
    ...simulation.wires.map((wire, index) =>
      [
        "GW",
        index + 1,
        wire.segments,
        ...nec(wire.from, lift),
        ...nec(wire.to, lift),
        number(simulation.wire_radius_m),
      ].join(" "),
    ),
    `GE ${grounded ? 1 : 0}`,
    ground(model.ground),
    ...(conductivity === null ? [] : [`LD 5 0 0 0 ${conductivity.toExponential(2)}`]),
    `EX 0 ${simulation.feed_wire + 1} ${simulation.feed_segment + 1} 0 1 0`,
    `FR 0 ${sweep.length} 0 0 ${number(first / 1e6)} ${number(step / 1e6)}`,
    grounded ? "RP 0 19 73 1000 0 0 5 5" : "RP 0 37 73 1000 0 0 5 5",
    "EN",
    "",
  ].join("\n");
}

function ground(kind: AntennaGround): string {
  if (kind === "free_space") {
    return "GN -1";
  }
  if (kind === "perfect") {
    return "GN 1";
  }
  const [permittivity, conductivity] = SOIL[kind];
  return `GN 2 0 0 0 ${permittivity} ${conductivity}`;
}

function nec(point: AntennaPoint, lift: number): string[] {
  return [point.z_m, point.x_m, point.y_m + lift].map(number);
}

function number(value: number): string {
  const rounded = Math.abs(value) < 1e-9 ? 0 : value;
  return Number(rounded.toPrecision(7)).toString();
}

export function necFilename(report: AntennaReport): string {
  const name = report.design.type.replaceAll("_", "-");
  return `${name}-${Math.round(report.frequency_hz / 1000)}khz.nec`;
}
