import type { Options } from "../../components/controls";
import { SCHEDULE_LIMITS } from "../../lib/limits";
import type { ScheduleStatus, ScheduleWindow, Weekday } from "../../lib/types";

const WEEK: readonly { value: Weekday; label: string; title: string }[] = [
  { value: "monday", label: "M", title: "Monday" },
  { value: "tuesday", label: "T", title: "Tuesday" },
  { value: "wednesday", label: "W", title: "Wednesday" },
  { value: "thursday", label: "T", title: "Thursday" },
  { value: "friday", label: "F", title: "Friday" },
  { value: "saturday", label: "S", title: "Saturday" },
  { value: "sunday", label: "S", title: "Sunday" },
];

export const WEEKDAYS: Options<Weekday> = WEEK;

const DAY_MS = 86_400_000;

export function clockText(minutes: number): string {
  const hours = Math.floor(minutes / 60);
  return `${String(hours).padStart(2, "0")}:${String(minutes % 60).padStart(2, "0")}`;
}

export function parsedClock(text: string): number | null {
  const found = /^(\d{1,2}):(\d{2})$/.exec(text.trim());
  if (found === null) {
    return null;
  }
  const minutes = Number(found[1]) * 60 + Number(found[2]);
  return Number(found[2]) < 60 && minutes < SCHEDULE_LIMITS.minutes_per_day ? minutes : null;
}

export function daysOn(window: ScheduleWindow): Weekday[] {
  const days = window.days ?? [];
  return days.length === 0 ? WEEK.map((day) => day.value) : days;
}

export function withDays(window: ScheduleWindow, picked: readonly Weekday[]): ScheduleWindow {
  const days = WEEK.map((day) => day.value).filter((day) => picked.includes(day));
  return { ...window, days: days.length === WEEK.length ? [] : days };
}

export function newWindow(): ScheduleWindow {
  return { start_min: 8 * 60, stop_min: 9 * 60, days: [] };
}

export function stateLine(
  status: ScheduleStatus | null,
  windows: number,
  nowMs: number,
): string | undefined {
  if (windows === 0) {
    return "no windows";
  }
  if (status === null || status.error != null) {
    return undefined;
  }
  const state = status.on ? "on" : "off";
  const next = status.next_change;
  return next == null ? state : `${state} until ${whenText(next * 1_000, nowMs, status.zone)}`;
}

function whenText(atMs: number, nowMs: number, zone: string | null | undefined): string {
  const timeZone = zone ?? undefined;
  const time = new Intl.DateTimeFormat(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
    timeZone,
  }).format(atMs);
  if (atMs - nowMs < DAY_MS) {
    return time;
  }
  const day = new Intl.DateTimeFormat(undefined, { weekday: "short", timeZone }).format(atMs);
  return `${day} ${time}`;
}
