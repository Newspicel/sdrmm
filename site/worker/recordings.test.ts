import { describe, expect, it } from "vitest";
import { DONATION_FILE, PART_BYTES, ROUTES } from "../src/recordings";
import worker from "./index";

interface Options {
  allowed?: boolean;
  mailFails?: boolean;
}

function bucket() {
  const stored = new Map<string, string>();
  const started: string[] = [];
  const uploaded: { key: string; partNumber: number; bytes: number }[] = [];
  const completed: string[] = [];
  const aborted: string[] = [];
  const resume = (key: string, uploadId: string) => ({
    key,
    uploadId,
    uploadPart: async (partNumber: number, value: ReadableStream) => {
      const bytes = (await new Response(value).arrayBuffer()).byteLength;
      uploaded.push({ key, partNumber, bytes });
      return { partNumber, etag: `e${partNumber}` };
    },
    complete: async () => {
      completed.push(key);
      stored.set(key, "");
      return {};
    },
    abort: async () => {
      aborted.push(key);
    },
  });
  const binding = {
    put: async (key: string, value: string) => {
      stored.set(key, value);
    },
    createMultipartUpload: async (key: string) => {
      started.push(key);
      return resume(key, `id-${started.length}`);
    },
    resumeMultipartUpload: resume,
    get: async (key: string) => {
      const value = stored.get(key);
      return value === undefined ? null : { json: async () => JSON.parse(value) };
    },
    list: async ({ prefix }: { prefix: string }) => ({
      objects: [...stored.keys()].filter((key) => key.startsWith(prefix)).map((key) => ({ key })),
    }),
  };
  return { binding, stored, started, uploaded, completed, aborted };
}

function environment({ allowed = true, mailFails = false }: Options = {}) {
  const r2 = bucket();
  const sent: EmailMessageBuilder[] = [];
  const env = {
    PRIVATE: r2.binding,
    RECORDINGS_TO: "contact@sdrmm.com",
    CONTACT_FROM: "contact@sdrmm.com",
    EMAIL: {
      send: async (message: EmailMessageBuilder) => {
        if (mailFails) {
          throw new Error("not verified");
        }
        sent.push(message);
        return { messageId: "1" };
      },
    },
    UPLOAD_LIMIT: { limit: async () => ({ success: allowed }) },
    ASSETS: { fetch: async () => new Response(null, { status: 404 }) },
  } as unknown as Env;
  return { env, r2, sent };
}

const key = "recordings/2026-10-04/123e4567-e89b-12d3-a456-426614174000/a.wav";
const donation = {
  signal: "POCSAG",
  notes: "",
  email: "",
  website: "",
  files: [{ name: "a.wav", size: 10 }],
};

function post(path: string, body: BodyInit | null, headers: Record<string, string> = {}) {
  return new Request(`https://sdrmm.com${path}`, {
    method: "POST",
    body,
    headers: { origin: "https://sdrmm.com", ...headers },
  });
}

async function send(request: Request, env: Env) {
  return worker.fetch(request as Parameters<typeof worker.fetch>[0], env);
}

function partPath(part: number, uploadKey = key) {
  return `${ROUTES.part}?${new URLSearchParams({ key: uploadKey, uploadId: "u", part: String(part) })}`;
}

describe("recording donations", () => {
  it("stores the donation and starts one upload per file", async () => {
    const { env, r2 } = environment();
    const files = [...donation.files, { name: "b.wav", size: 5 }];
    const response = await send(post(ROUTES.start, JSON.stringify({ ...donation, files })), env);
    expect(response.status).toBe(200);
    const body = (await response.json()) as { uploads: { key: string; uploadId: string }[] };
    expect(body.uploads.map((upload) => upload.key.split("/").at(-1))).toEqual(["a.wav", "b.wav"]);
    const [meta] = [...r2.stored.keys()];
    expect(meta?.endsWith(DONATION_FILE)).toBe(true);
    expect(JSON.parse(r2.stored.get(meta ?? "") ?? "")).toMatchObject({ signal: "POCSAG" });
  });

  it("rejects an invalid or trapped donation", async () => {
    const { env, r2 } = environment();
    expect((await send(post(ROUTES.start, "{}"), env)).status).toBe(400);
    const trapped = JSON.stringify({ ...donation, website: "spam" });
    expect((await send(post(ROUTES.start, trapped), env)).status).toBe(400);
    expect(r2.started).toHaveLength(0);
  });

  it("turns away a sender over the rate limit", async () => {
    const { env, r2 } = environment({ allowed: false });
    const response = await send(post(ROUTES.start, JSON.stringify(donation)), env);
    expect(response.status).toBe(429);
    expect(r2.started).toHaveLength(0);
  });

  it("streams a part into the bucket", async () => {
    const { env, r2 } = environment();
    const response = await send(
      post(partPath(2), new Uint8Array(10), { "content-length": "10" }),
      env,
    );
    expect(await response.json()).toEqual({ ok: true, part: { partNumber: 2, etag: "e2" } });
    expect(r2.uploaded).toEqual([{ key, partNumber: 2, bytes: 10 }]);
  });

  it("refuses oversized parts and keys outside the recordings folder", async () => {
    const { env, r2 } = environment();
    const big = { "content-length": String(PART_BYTES + 1) };
    expect((await send(post(partPath(1), new Uint8Array(1), big), env)).status).toBe(413);
    expect((await send(post(partPath(1, "public/x"), new Uint8Array(1)), env)).status).toBe(400);
    expect((await send(post(partPath(0), new Uint8Array(1)), env)).status).toBe(400);
    expect(r2.uploaded).toHaveLength(0);
  });

  it("completes and aborts uploads", async () => {
    const { env, r2 } = environment();
    const parts = [{ partNumber: 1, etag: "e1" }];
    const finish = post(ROUTES.finish, JSON.stringify({ key, uploadId: "u", parts }));
    expect((await send(finish, env)).status).toBe(200);
    expect(r2.completed).toEqual([key]);
    const query = new URLSearchParams({ key, uploadId: "u" });
    expect((await send(post(`${ROUTES.abort}?${query}`, null), env)).status).toBe(200);
    expect(r2.aborted).toEqual([key]);
  });

  it("mails once when every file of a donation is complete", async () => {
    const { env, sent } = environment();
    const files = [...donation.files, { name: "b.wav", size: 2_500_000 }];
    const body = { ...donation, email: "ada@example.org", notes: "Strong", files };
    const started = await send(post(ROUTES.start, JSON.stringify(body)), env);
    const { uploads } = (await started.json()) as { uploads: { key: string; uploadId: string }[] };
    for (const upload of uploads) {
      const parts = [{ partNumber: 1, etag: "e1" }];
      await send(post(ROUTES.finish, JSON.stringify({ ...upload, parts })), env);
    }
    expect(sent).toHaveLength(1);
    expect(sent[0]).toMatchObject({
      to: "contact@sdrmm.com",
      replyTo: "ada@example.org",
      subject: "SDR-- recording: POCSAG",
    });
    expect(sent[0]?.text).toContain("b.wav (2.5 MB)");
    expect(sent[0]?.text).toContain("Strong");
  });

  it("keeps the upload when the mail fails", async () => {
    const { env, r2 } = environment({ mailFails: true });
    const started = await send(post(ROUTES.start, JSON.stringify(donation)), env);
    const { uploads } = (await started.json()) as { uploads: { key: string; uploadId: string }[] };
    const parts = [{ partNumber: 1, etag: "e1" }];
    const finish = post(ROUTES.finish, JSON.stringify({ ...uploads[0], parts }));
    expect((await send(finish, env)).status).toBe(200);
    expect(r2.completed).toHaveLength(1);
  });

  it("reports a bucket failure", async () => {
    const { env } = environment();
    const failing = {
      ...env,
      PRIVATE: { put: async () => Promise.reject(new Error("down")) },
    } as unknown as Env;
    const response = await send(post(ROUTES.start, JSON.stringify(donation)), failing);
    expect(response.status).toBe(502);
  });

  it("refuses posts from other sites and other methods", async () => {
    const { env } = environment();
    const foreign = post(ROUTES.start, JSON.stringify(donation), {
      origin: "https://evil.example",
    });
    expect((await send(foreign, env)).status).toBe(403);
    expect((await send(new Request(`https://sdrmm.com${ROUTES.start}`), env)).status).toBe(405);
  });
});
