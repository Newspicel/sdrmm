import type { Hue, PatchCatalog, PortType } from "../lib/types";

export const HUE_TEXT: Record<Hue, string> = {
  accent: "text-accent",
  muted: "text-ink-faint",
  blue: "text-hue-blue",
  cyan: "text-hue-cyan",
  teal: "text-hue-teal",
  green: "text-hue-green",
  amber: "text-hue-amber",
  orange: "text-hue-orange",
  violet: "text-hue-violet",
  indigo: "text-hue-indigo",
  pink: "text-hue-pink",
};

export const HUE_COLOR: Record<Hue, string> = {
  accent: "var(--color-accent)",
  muted: "var(--color-ink-faint)",
  blue: "var(--color-hue-blue)",
  cyan: "var(--color-hue-cyan)",
  teal: "var(--color-hue-teal)",
  green: "var(--color-hue-green)",
  amber: "var(--color-hue-amber)",
  orange: "var(--color-hue-orange)",
  violet: "var(--color-hue-violet)",
  indigo: "var(--color-hue-indigo)",
  pink: "var(--color-hue-pink)",
};

export const HUE_TOKEN: Record<Hue, string> = {
  accent: "--color-accent",
  muted: "--color-ink-faint",
  blue: "--color-hue-blue",
  cyan: "--color-hue-cyan",
  teal: "--color-hue-teal",
  green: "--color-hue-green",
  amber: "--color-hue-amber",
  orange: "--color-hue-orange",
  violet: "--color-hue-violet",
  indigo: "--color-hue-indigo",
  pink: "--color-hue-pink",
};

export const HUES = Object.keys(HUE_TOKEN) as Hue[];

export function portHue(catalog: PatchCatalog, portType: PortType): Hue {
  return catalog.port_types.find((info) => info.port_type === portType)?.hue ?? "muted";
}
