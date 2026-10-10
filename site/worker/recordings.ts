import {
  DONATION_FILE,
  type Donation,
  donationFolder,
  PART_BYTES,
  parseFinish,
  parsePartNumber,
  parseStart,
  parseUploadQuery,
  ROUTES,
  safeName,
  type Upload,
} from "../src/recordings";
import { notifyIfDone } from "./notify";

const MAX_JSON = 64 * 1024;

function crossOrigin(request: Request): boolean {
  const origin = request.headers.get("origin");
  return origin !== null && origin !== new URL(request.url).origin;
}

function fail(status: number): Response {
  return Response.json({ ok: false }, { status });
}

async function json(request: Request): Promise<unknown> {
  if (Number(request.headers.get("content-length") ?? 0) > MAX_JSON) {
    return null;
  }
  return request.json().catch(() => null);
}

async function createUploads(donation: Donation, env: Env): Promise<Upload[]> {
  const date = new Date();
  const folder = donationFolder(date, crypto.randomUUID());
  await env.PRIVATE.put(
    `${folder}${DONATION_FILE}`,
    JSON.stringify({ ...donation, date: date.toISOString() }),
    {
      httpMetadata: { contentType: "application/json" },
    },
  );
  return Promise.all(
    donation.files.map(async (file) => {
      const upload = await env.PRIVATE.createMultipartUpload(`${folder}${safeName(file.name)}`);
      return { key: upload.key, uploadId: upload.uploadId };
    }),
  );
}

async function start(request: Request, env: Env): Promise<Response> {
  const client = request.headers.get("cf-connecting-ip") ?? "unknown";
  const { success } = await env.UPLOAD_LIMIT.limit({ key: client });
  if (!success) {
    return fail(429);
  }
  const parsed = parseStart(await json(request));
  if (parsed.kind === "trapped") {
    console.warn("recordings: trap field filled, donation dropped");
  }
  if (parsed.kind !== "donation") {
    return fail(400);
  }
  return Response.json({ ok: true, uploads: await createUploads(parsed.donation, env) });
}

async function part(request: Request, env: Env, url: URL): Promise<Response> {
  const upload = parseUploadQuery(url.searchParams);
  const partNumber = parsePartNumber(url.searchParams.get("part"));
  const length = Number(request.headers.get("content-length") ?? 0);
  if (upload === null || partNumber === null || request.body === null) {
    return fail(400);
  }
  if (length <= 0 || length > PART_BYTES) {
    return fail(413);
  }
  const multipart = env.PRIVATE.resumeMultipartUpload(upload.key, upload.uploadId);
  const uploaded = await multipart.uploadPart(partNumber, request.body);
  return Response.json({ ok: true, part: uploaded });
}

async function finish(request: Request, env: Env): Promise<Response> {
  const parsed = parseFinish(await json(request));
  if (parsed === null) {
    return fail(400);
  }
  const multipart = env.PRIVATE.resumeMultipartUpload(parsed.key, parsed.uploadId);
  await multipart.complete(parsed.parts);
  await notifyIfDone(parsed.key, env);
  return Response.json({ ok: true });
}

async function abort(env: Env, url: URL): Promise<Response> {
  const upload = parseUploadQuery(url.searchParams);
  if (upload === null) {
    return fail(400);
  }
  await env.PRIVATE.resumeMultipartUpload(upload.key, upload.uploadId).abort();
  return Response.json({ ok: true });
}

function route(request: Request, env: Env, url: URL): Promise<Response> | Response {
  switch (url.pathname) {
    case ROUTES.start:
      return start(request, env);
    case ROUTES.part:
      return part(request, env, url);
    case ROUTES.finish:
      return finish(request, env);
    case ROUTES.abort:
      return abort(env, url);
    default:
      return fail(404);
  }
}

export async function recordings(request: Request, env: Env): Promise<Response> {
  if (request.method !== "POST") {
    return new Response(null, { status: 405, headers: { Allow: "POST" } });
  }
  if (crossOrigin(request)) {
    return fail(403);
  }
  const url = new URL(request.url);
  try {
    return await route(request, env, url);
  } catch (error) {
    console.error(`recordings: ${url.pathname} failed`, error);
    return fail(502);
  }
}
