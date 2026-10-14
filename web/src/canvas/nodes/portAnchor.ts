export interface Span {
  top: number;
  height: number;
}

export type Anchors = Readonly<Record<string, number>>;

export function visualCenter(mark: Span, host: Span, scale: number, border: number): number {
  return (mark.top + mark.height / 2 - host.top) / scale - border;
}

export function anchorsOf<T>(marks: ReadonlyMap<string, T>, centre: (mark: T) => number): Anchors {
  return Object.fromEntries([...marks].map(([port, mark]) => [port, centre(mark)]));
}

export function sameAnchors(a: Anchors, b: Anchors): boolean {
  const ports = Object.keys(a);
  return ports.length === Object.keys(b).length && ports.every((port) => a[port] === b[port]);
}
