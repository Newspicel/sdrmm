export const RECORDINGS_PATH = "/api/recordings";

export const ROUTES = {
  start: `${RECORDINGS_PATH}/start`,
  part: `${RECORDINGS_PATH}/part`,
  finish: `${RECORDINGS_PATH}/finish`,
  abort: `${RECORDINGS_PATH}/abort`,
} as const;

export const PART_BYTES = 16 * 1024 * 1024;
export const MAX_FILE_BYTES = 16 * 1024 * 1024 * 1024;
export const MAX_PARTS = Math.ceil(MAX_FILE_BYTES / PART_BYTES);
export const MAX_FILES = 10;

export const LIMITS = {
  signal: 200,
  notes: 1000,
  email: 254,
  file: 200,
} as const;

export const KEY_PREFIX = "recordings/";

export interface FileEntry {
  name: string;
  size: number;
}

export interface Donation {
  signal: string;
  notes: string;
  email: string;
  files: FileEntry[];
}

export interface StartRequest extends Donation {
  website: string;
}

export interface Upload {
  key: string;
  uploadId: string;
}

export interface UploadedPart {
  partNumber: number;
  etag: string;
}

export interface FinishRequest extends Upload {
  parts: UploadedPart[];
}

export type ParsedStart =
  | { kind: "donation"; donation: Donation }
  | { kind: "invalid" }
  | { kind: "trapped" };

const EMAIL = /^[^\s@<>"(),;:]+@[^\s@<>"(),;:]+\.[^\s@<>"(),;:]+$/;
const CONTROL = /\p{Cc}/u;
const UNSAFE_NAME = /[^\w.+-]+/g;

function record(value: unknown): Record<string, unknown> {
  return typeof value === "object" && value !== null ? (value as Record<string, unknown>) : {};
}

function text(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

function fits(value: string, limit: number): boolean {
  return value.length <= limit && !CONTROL.test(value.replace(/[\n\r\t]/g, " "));
}

function singleLine(value: string, limit: number): boolean {
  return value.length <= limit && !CONTROL.test(value);
}

export function safeName(name: string): string {
  const cleaned = name.replace(UNSAFE_NAME, "_").replace(/^[._]+/, "");
  return cleaned.slice(-LIMITS.file) || "recording";
}

export function partCount(size: number): number {
  return Math.max(1, Math.ceil(size / PART_BYTES));
}

function parseFile(value: unknown): FileEntry | null {
  const entry = record(value);
  const name = text(entry.name);
  const size = entry.size;
  const valid =
    name.length > 0 &&
    singleLine(name, 1024) &&
    typeof size === "number" &&
    Number.isSafeInteger(size) &&
    size > 0 &&
    size <= MAX_FILE_BYTES;
  return valid ? { name, size } : null;
}

function parseFiles(value: unknown): FileEntry[] | null {
  if (!Array.isArray(value) || value.length === 0 || value.length > MAX_FILES) {
    return null;
  }
  const files = value.map(parseFile);
  if (!files.every((file) => file !== null)) {
    return null;
  }
  const names = new Set(files.map((file) => safeName(file.name)));
  return names.size === files.length ? files : null;
}

export function parseStart(body: unknown): ParsedStart {
  const fields = record(body);
  if (text(fields.website) !== "") {
    return { kind: "trapped" };
  }
  const signal = text(fields.signal);
  const notes = text(fields.notes);
  const email = text(fields.email);
  const files = parseFiles(fields.files);
  const valid =
    files !== null &&
    signal.length > 0 &&
    singleLine(signal, LIMITS.signal) &&
    fits(notes, LIMITS.notes) &&
    singleLine(email, LIMITS.email) &&
    (email === "" || EMAIL.test(email));
  return valid
    ? { kind: "donation", donation: { signal, notes, email, files } }
    : { kind: "invalid" };
}

export function isDonationKey(key: string): boolean {
  return /^recordings\/\d{4}-\d{2}-\d{2}\/[0-9a-f-]{36}\/[\w.+-]+$/.test(key);
}

function parseUpload(fields: Record<string, unknown>): Upload | null {
  const key = text(fields.key);
  const uploadId = text(fields.uploadId);
  return isDonationKey(key) && uploadId.length > 0 ? { key, uploadId } : null;
}

export function parseUploadQuery(params: URLSearchParams): Upload | null {
  return parseUpload({ key: params.get("key"), uploadId: params.get("uploadId") });
}

export function parsePartNumber(value: string | null): number | null {
  const number = Number(value);
  return Number.isInteger(number) && number >= 1 && number <= MAX_PARTS ? number : null;
}

function parsePart(value: unknown): UploadedPart | null {
  const part = record(value);
  const partNumber = parsePartNumber(String(part.partNumber));
  const etag = text(part.etag);
  return partNumber !== null && etag.length > 0 ? { partNumber, etag } : null;
}

export function parseFinish(body: unknown): FinishRequest | null {
  const fields = record(body);
  const upload = parseUpload(fields);
  const parts = Array.isArray(fields.parts) ? fields.parts.map(parsePart) : [];
  if (upload === null || parts.length === 0 || parts.length > MAX_PARTS) {
    return null;
  }
  return parts.every((part) => part !== null) ? { ...upload, parts } : null;
}

export const DONATION_FILE = "_donation.json";

export function donationFolder(date: Date, id: string): string {
  return `${KEY_PREFIX}${date.toISOString().slice(0, 10)}/${id}/`;
}
