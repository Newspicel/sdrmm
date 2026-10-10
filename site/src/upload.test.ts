import { describe, expect, it } from "vitest";
import { PART_BYTES, ROUTES } from "./recordings";
import { donate, type Fetch, UploadFailed } from "./upload";

interface Call {
  path: string;
  params: URLSearchParams;
  body: unknown;
}

const fields = { signal: "POCSAG", notes: "", email: "" };

function server(respond: (call: Call) => Response | null = () => null) {
  const calls: Call[] = [];
  const fetch: Fetch = async (url, init) => {
    const parsed = new URL(url, "https://sdrmm.com");
    const body = typeof init.body === "string" ? JSON.parse(init.body) : init.body;
    const call = { path: parsed.pathname, params: parsed.searchParams, body };
    calls.push(call);
    const custom = respond(call);
    if (custom !== null) {
      return custom;
    }
    if (call.path === ROUTES.start) {
      const files = (body as { files: unknown[] }).files;
      const uploads = files.map((_, i) => ({ key: `k${i}`, uploadId: `u${i}` }));
      return Response.json({ ok: true, uploads });
    }
    if (call.path === ROUTES.part) {
      const partNumber = Number(call.params.get("part"));
      return Response.json({ ok: true, part: { partNumber, etag: `e${partNumber}` } });
    }
    return Response.json({ ok: true });
  };
  return { calls, fetch };
}

function sized(name: string, size: number): File {
  return new File([new Uint8Array(size)], name);
}

describe("donate", () => {
  it("uploads every part and finishes with parts in order", async () => {
    const { calls, fetch } = server();
    const progress: number[] = [];
    const file = sized("a.sigmf-data", PART_BYTES * 2 + 5);
    await donate(fields, [file], "", { fetch, onProgress: (sent) => progress.push(sent) });
    const parts = calls.filter((call) => call.path === ROUTES.part);
    expect(parts.map((call) => call.params.get("part")).toSorted()).toEqual(["1", "2", "3"]);
    expect((parts[0]?.body as Blob).size).toBeLessThanOrEqual(PART_BYTES);
    const finish = calls.find((call) => call.path === ROUTES.finish);
    expect(finish?.body).toEqual({
      key: "k0",
      uploadId: "u0",
      parts: [1, 2, 3].map((n) => ({ partNumber: n, etag: `e${n}` })),
    });
    expect(progress.at(-1)).toBe(file.size);
  });

  it("sends the donation fields and file list on start", async () => {
    const { calls, fetch } = server();
    await donate(fields, [sized("a.wav", 3), sized("b.wav", 4)], "", {
      fetch,
      onProgress: () => {},
    });
    expect(calls[0]?.body).toEqual({
      ...fields,
      website: "",
      files: [
        { name: "a.wav", size: 3 },
        { name: "b.wav", size: 4 },
      ],
    });
    expect(calls.filter((call) => call.path === ROUTES.finish)).toHaveLength(2);
  });

  it("retries a failed part", async () => {
    let failures = 1;
    const { calls, fetch } = server((call) =>
      call.path === ROUTES.part && failures-- > 0 ? new Response(null, { status: 502 }) : null,
    );
    await donate(fields, [sized("a.wav", 10)], "", { fetch, onProgress: () => {} });
    expect(calls.filter((call) => call.path === ROUTES.part)).toHaveLength(2);
  });

  it("aborts the uploads when a part keeps failing", async () => {
    const { calls, fetch } = server((call) =>
      call.path === ROUTES.part ? new Response(null, { status: 502 }) : null,
    );
    const upload = donate(fields, [sized("a.wav", 10)], "", { fetch, onProgress: () => {} });
    await expect(upload).rejects.toEqual(new UploadFailed("failed"));
    expect(calls.filter((call) => call.path === ROUTES.part)).toHaveLength(3);
    expect(calls.at(-1)?.path).toBe(ROUTES.abort);
  });

  it("reports a rate limited start", async () => {
    const { fetch } = server(() => new Response(null, { status: 429 }));
    const upload = donate(fields, [sized("a.wav", 1)], "", { fetch, onProgress: () => {} });
    await expect(upload).rejects.toMatchObject({ reason: "busy" });
  });
});
