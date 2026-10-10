export interface TimeTick {
  y: number;
  label: string;
}

const STEPS_S = [1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 1800, 3600];

export function timeTicks(
  timeAt: (rowsBack: number) => number,
  rows: number,
  height: number,
  minGap: number,
): TimeTick[] {
  const deepest = deepestKnown(timeAt, rows);
  const spanMs = deepest === null ? 0 : timeAt(0) - timeAt(deepest);
  if (deepest === null || !(spanMs > 0)) {
    return [];
  }
  const rowPx = height / Math.max(1, rows - 1);
  const pxPerSecond = (deepest * rowPx * 1000) / spanMs;
  const step = STEPS_S.find((seconds) => seconds * pxPerSecond >= minGap);
  if (step === undefined) {
    return [];
  }
  const stepMs = step * 1000;
  const ticks: TimeTick[] = [];
  for (let back = 0; back < deepest; back++) {
    const newer = Math.floor(timeAt(back) / stepMs);
    const older = Math.floor(timeAt(back + 1) / stepMs);
    if (Number.isFinite(newer) && Number.isFinite(older) && newer !== older) {
      ticks.push({ y: back * rowPx, label: formatClock(newer * stepMs, step < 60) });
    }
  }
  return ticks;
}

export function formatClock(at: number, seconds: boolean): string {
  const time = new Date(at);
  const parts = [time.getHours(), time.getMinutes()];
  if (seconds) {
    parts.push(time.getSeconds());
  }
  return parts.map((part) => String(part).padStart(2, "0")).join(":");
}

function deepestKnown(timeAt: (rowsBack: number) => number, rows: number): number | null {
  if (!Number.isFinite(timeAt(0))) {
    return null;
  }
  for (let back = rows - 1; back > 0; back--) {
    if (Number.isFinite(timeAt(back))) {
      return back;
    }
  }
  return null;
}
