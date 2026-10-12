import type { NanoVnaPoint } from "../../lib/types";
import { REFERENCE_COLORS } from "./views";

export interface Reference {
  key: string;
  label: string;
  color: string;
  points: readonly NanoVnaPoint[];
}

export type ReferenceSource = Omit<Reference, "color">;

export const MAX_REFERENCES = REFERENCE_COLORS.length;

export function toggleReference(
  references: readonly Reference[],
  source: ReferenceSource,
): Reference[] {
  if (references.some((reference) => reference.key === source.key)) {
    return references.filter((reference) => reference.key !== source.key);
  }
  const color = REFERENCE_COLORS.find((slot) => references.every((entry) => entry.color !== slot));
  if (color === undefined) {
    return [...references];
  }
  return [...references, { ...source, color }];
}

export function dropReference(references: readonly Reference[], key: string): Reference[] {
  return references.filter((reference) => reference.key !== key);
}

export function storedKey(id: number): string {
  return `stored:${id}`;
}
