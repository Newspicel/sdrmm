import {
  type Donation,
  PART_BYTES,
  partCount,
  ROUTES,
  type StartRequest,
  type Upload,
  type UploadedPart,
} from "./recordings";

export type Fetch = (url: string, init: RequestInit) => Promise<Response>;

export type UploadError = "invalid" | "busy" | "failed";

export class UploadFailed extends Error {
  constructor(readonly reason: UploadError) {
    super(reason);
  }
}

export interface Options {
  fetch: Fetch;
  onProgress: (sent: number, total: number) => void;
  concurrency?: number;
  attempts?: number;
}

function reasonOf(status: number): UploadError {
  if (status === 400) {
    return "invalid";
  }
  return status === 429 ? "busy" : "failed";
}

async function post(options: Options, url: string, body: BodyInit, json = true): Promise<unknown> {
  const headers = json ? { "content-type": "application/json" } : undefined;
  const response = await options.fetch(url, { method: "POST", body, headers }).catch(() => null);
  if (response === null) {
    throw new UploadFailed("failed");
  }
  if (!response.ok) {
    throw new UploadFailed(reasonOf(response.status));
  }
  return response.json();
}

function query(upload: Upload, extra: Record<string, string> = {}): string {
  return new URLSearchParams({ key: upload.key, uploadId: upload.uploadId, ...extra }).toString();
}

async function start(options: Options, request: StartRequest): Promise<Upload[]> {
  const body = await post(options, ROUTES.start, JSON.stringify(request));
  const uploads = (body as { uploads?: Upload[] }).uploads;
  if (!Array.isArray(uploads) || uploads.length !== request.files.length) {
    throw new UploadFailed("failed");
  }
  return uploads;
}

async function sendPart(options: Options, upload: Upload, file: Blob, index: number) {
  const chunk = file.slice(index * PART_BYTES, (index + 1) * PART_BYTES);
  const url = `${ROUTES.part}?${query(upload, { part: String(index + 1) })}`;
  const attempts = options.attempts ?? 3;
  for (let attempt = 1; ; attempt++) {
    try {
      const body = await post(options, url, chunk, false);
      return { part: (body as { part: UploadedPart }).part, bytes: chunk.size };
    } catch (error) {
      if (attempt >= attempts || (error instanceof UploadFailed && error.reason !== "failed")) {
        throw error;
      }
    }
  }
}

async function sendFile(
  options: Options,
  upload: Upload,
  file: Blob,
  onPart: (bytes: number) => void,
) {
  const count = partCount(file.size);
  const parts: UploadedPart[] = [];
  let next = 0;
  const worker = async () => {
    while (next < count) {
      const { part, bytes } = await sendPart(options, upload, file, next++);
      parts.push(part);
      onPart(bytes);
    }
  };
  const workers = Math.min(options.concurrency ?? 4, count);
  await Promise.all(Array.from({ length: workers }, worker));
  parts.sort((a, b) => a.partNumber - b.partNumber);
  await post(options, ROUTES.finish, JSON.stringify({ ...upload, parts }));
}

async function abort(options: Options, uploads: Upload[]) {
  await Promise.all(
    uploads.map((upload) =>
      options.fetch(`${ROUTES.abort}?${query(upload)}`, { method: "POST" }).catch(() => null),
    ),
  );
}

export async function donate(
  fields: Omit<Donation, "files">,
  files: readonly File[],
  website: string,
  options: Options,
): Promise<void> {
  const entries = files.map((file) => ({ name: file.name, size: file.size }));
  const uploads = await start(options, { ...fields, website, files: entries });
  const total = files.reduce((sum, file) => sum + file.size, 0);
  let sent = 0;
  const onPart = (bytes: number) => {
    sent += bytes;
    options.onProgress(sent, total);
  };
  try {
    for (const [index, file] of files.entries()) {
      const upload = uploads[index];
      if (upload !== undefined) {
        await sendFile(options, upload, file, onPart);
      }
    }
  } catch (error) {
    await abort(options, uploads);
    throw error;
  }
}
