import { createToaster } from "@ui-sdk/react/toast";
import { describeError, recordEvent } from "./diagnostics";
import type { ErrorCode } from "./types";

export type Tone = "error" | "info";

export interface ToastData {
  repeats: number;
  code?: ErrorCode;
  detail?: string;
}

const LIFETIME_MS = 12_000;

export const toaster = createToaster<string>({ duration: LIFETIME_MS });

const repeats = new Map<string, number>();

export interface ToastDetail {
  code?: ErrorCode;
  detail?: string;
}

export function pushToast(message: string, tone: Tone = "error", extra: ToastDetail = {}): void {
  const id = `${tone}:${message}`;
  const seen = (repeats.get(id) ?? -1) + 1;
  repeats.set(id, seen);
  recordEvent(tone === "error" ? "error" : "info", "toast", toastRecord(message, extra));
  const data: ToastData = { repeats: seen, code: extra.code, detail: extra.detail };
  toaster({ id, type: tone, title: message, data, onDismiss: () => repeats.delete(id) });
}

export function closeToast(message: string, tone: Tone = "error"): void {
  toaster.dismiss(`${tone}:${message}`);
}

function toastRecord(message: string, extra: ToastDetail): string {
  const code = extra.code === undefined ? "" : `[${extra.code}] `;
  const detail = extra.detail === undefined ? "" : `: ${extra.detail}`;
  return `${code}${message}${detail}`;
}

export function toastError(error: unknown, tone: Tone = "error"): void {
  const carried = error as { code?: ErrorCode; detail?: string } | null;
  pushToast(messageOf(error), tone, {
    code: carried?.code,
    detail: carried?.detail,
  });
}

export function toastRejection(task: Promise<unknown>): void {
  task.catch((error: unknown) => toastError(error));
}

function messageOf(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  return describeError(error);
}
