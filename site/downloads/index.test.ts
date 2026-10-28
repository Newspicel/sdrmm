import { afterEach, describe, expect, it, vi } from "vitest";
import worker, { kind } from "./index";

const HOST = "https://downloads.sdrmm.com";

function environment() {
  const points: AnalyticsEngineDataPoint[] = [];
  const env = {
    DOWNLOADS: { writeDataPoint: (point: AnalyticsEngineDataPoint) => points.push(point) },
  } as unknown as Env;
  return { env, points };
}

function origin(status = 200) {
  const requested: string[] = [];
  vi.stubGlobal("fetch", async (request: Request) => {
    requested.push(request.url);
    return new Response("body", { status });
  });
  return requested;
}

async function send(path: string, env: Env, init: RequestInit = {}) {
  const request = new Request(`${HOST}${path}`, init);
  return worker.fetch(request as Parameters<typeof worker.fetch>[0], env);
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("download counter", () => {
  it("sorts paths by what they mean", () => {
    expect(kind("/releases/latest.json")).toBe("update");
    expect(kind("/releases/v2.2.1/SDRmm_2.2.1_aarch64.dmg")).toBe("download");
    expect(kind("/packages/deb/dists/stable/InRelease")).toBe("repo");
    expect(kind("/packages/rpm/repodata/repomd.xml")).toBe("repo");
    expect(kind("/packages/deb/pool/main/sdrmm_2.2.1_amd64.deb")).toBe("package");
    expect(kind("/packages/key.gpg")).toBe("other");
    expect(kind("/releases/changelog.json")).toBe("other");
  });

  it("counts an update check with its version and platform", async () => {
    const { env, points } = environment();
    const requested = origin();
    const query = "?version=2.2.0&target=darwin&arch=aarch64";
    const response = await send(`/releases/latest.json${query}`, env);
    expect(await response.text()).toBe("body");
    expect(requested).toEqual([`${HOST}/releases/latest.json`]);
    expect(points).toHaveLength(1);
    expect(points[0]?.indexes).toEqual(["update"]);
    expect(points[0]?.blobs?.slice(2, 5)).toEqual(["2.2.0", "darwin", "aarch64"]);
  });

  it("logs each counted hit as structured fields", async () => {
    const { env } = environment();
    origin();
    const log = vi.spyOn(console, "log").mockImplementation(() => {});
    await send("/releases/latest.json?version=2.2.0&target=linux&arch=x86_64", env);
    expect(log).toHaveBeenCalledWith(
      expect.objectContaining({
        message: "hit",
        kind: "update",
        file: "latest.json",
        target: "linux",
      }),
    );
    log.mockRestore();
  });

  it("takes the version of a download from its path", async () => {
    const { env, points } = environment();
    origin();
    await send("/releases/v2.2.1/SDRmm_2.2.1_x64-setup.exe", env);
    expect(points[0]?.blobs?.slice(0, 3)).toEqual([
      "download",
      "/releases/v2.2.1/SDRmm_2.2.1_x64-setup.exe",
      "v2.2.1",
    ]);
  });

  it("skips resumed ranges, HEAD requests and errors", async () => {
    const { env, points } = environment();
    origin();
    const file = "/releases/v2.2.1/a.dmg";
    await send(file, env, { headers: { range: "bytes=500-" } });
    await send(file, env, { method: "HEAD" });
    origin(404);
    await send(file, env);
    expect(points).toHaveLength(0);
  });

  it("counts a download that starts at the first byte", async () => {
    const { env, points } = environment();
    origin(206);
    await send("/releases/v2.2.1/a.dmg", env, { headers: { range: "bytes=0-" } });
    expect(points).toHaveLength(1);
  });
});
