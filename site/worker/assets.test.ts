import { describe, expect, it } from "vitest";
import { docsPath } from "./assets";
import worker from "./index";

function environment(pages: Record<string, string>) {
  return {
    ASSETS: {
      fetch: async (input: URL, init?: RequestInit) => {
        const body = pages[input.pathname];
        if (body === undefined) {
          return new Response("missing", { status: 404 });
        }
        return new Response(init?.method === "HEAD" ? null : body, {
          headers: { "content-type": "text/html" },
        });
      },
    },
  } as unknown as Env;
}

async function get(path: string, env: Env) {
  return worker.fetch(
    new Request(`https://sdrmm.com${path}`) as Parameters<typeof worker.fetch>[0],
    env,
  );
}

describe("docsPath", () => {
  it("maps old chapter addresses into the new docs paths", () => {
    expect(docsPath("/hardware")).toBe("/docs/radios");
    expect(docsPath("/hardware.html")).toBe("/docs/radios");
    expect(docsPath("/user-guide/decoders/")).toBe("/docs/operate/decoders");
    expect(docsPath("/docs/getting-started/install")).toBe("/docs/start/install");
    expect(docsPath("/docs/hardware.html")).toBe("/docs/radios");
  });

  it("leaves the root, current docs, and files alone", () => {
    expect(docsPath("/")).toBeNull();
    expect(docsPath("/docs")).toBeNull();
    expect(docsPath("/docs/nowhere")).toBeNull();
    expect(docsPath("/icon.svg")).toBeNull();
  });
});

describe("missing assets", () => {
  const env = environment({ "/docs/radios": "radios", "/404": "not found" });

  it("moves an old chapter into the docs, keeping the query", async () => {
    const response = await get("/hardware.html?from=readme", env);
    expect(response.status).toBe(301);
    expect(response.headers.get("location")).toBe("https://sdrmm.com/docs/radios?from=readme");
  });

  it("answers anything else with the 404 page", async () => {
    const response = await get("/nowhere", env);
    expect(response.status).toBe(404);
    expect(await response.text()).toBe("not found");
    expect(response.headers.get("content-type")).toBe("text/html");
  });
});
