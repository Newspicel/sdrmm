import type { PointReadout } from "./analysis";
import { nearestIndex } from "./views";

export const MAX_MARKERS = 4;

export interface Marker {
  id: number;
  hz: number;
}

export type ActiveMarker = number | "best";

export interface MarkerRow {
  id: ActiveMarker;
  label: string;
  row: PointReadout;
}

export function placeMarker(
  markers: readonly Marker[],
  active: ActiveMarker,
  hz: number,
): { markers: Marker[]; active: number } {
  if (active !== "best" && markers.some((marker) => marker.id === active)) {
    return {
      markers: markers.map((marker) => (marker.id === active ? { ...marker, hz } : marker)),
      active,
    };
  }
  if (markers.length >= MAX_MARKERS) {
    const last = markers[markers.length - 1] as Marker;
    return placeMarker(markers, last.id, hz);
  }
  const id = freeId(markers);
  return { markers: [...markers, { id, hz }].toSorted((a, b) => a.id - b.id), active: id };
}

export function stepMarker(
  markers: readonly Marker[],
  active: ActiveMarker,
  rows: readonly PointReadout[],
  delta: number,
): Marker[] {
  const marker = markers.find((entry) => entry.id === active);
  if (marker === undefined || rows.length === 0) {
    return [...markers];
  }
  const xs = rows.map((row) => row.frequencyHz);
  const index = Math.min(rows.length - 1, Math.max(0, nearestIndex(xs, marker.hz) + delta));
  const hz = rows[index]?.frequencyHz ?? marker.hz;
  return markers.map((entry) => (entry.id === active ? { ...entry, hz } : entry));
}

export function markerRows(
  markers: readonly Marker[],
  best: PointReadout | null,
  rows: readonly PointReadout[],
): MarkerRow[] {
  const xs = rows.map((row) => row.frequencyHz);
  const placed = markers.flatMap((marker) => {
    const row = rows[nearestIndex(xs, marker.hz)];
    return row === undefined ? [] : [{ id: marker.id, label: String(marker.id), row }];
  });
  return best === null ? placed : [{ id: "best", label: "▼", row: best }, ...placed];
}

function freeId(markers: readonly Marker[]): number {
  let id = 1;
  while (markers.some((marker) => marker.id === id)) {
    id += 1;
  }
  return id;
}
