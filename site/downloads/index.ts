export type Kind = "update" | "download" | "repo" | "package" | "other";

const UPDATE_FEED = "/releases/latest.json";
const REPO_INDEXES = ["/InRelease", "/Release", "/repomd.xml"];

export function kind(pathname: string): Kind {
  if (pathname === UPDATE_FEED) {
    return "update";
  }
  if (/^\/releases\/v[^/]+\/[^/]+$/.test(pathname)) {
    return "download";
  }
  if (!pathname.startsWith("/packages/")) {
    return "other";
  }
  if (REPO_INDEXES.some((index) => pathname.endsWith(index))) {
    return "repo";
  }
  return /\.(deb|rpm)$/.test(pathname) ? "package" : "other";
}

export function counted(request: Request, response: Response): boolean {
  const range = request.headers.get("range");
  const fromStart = range === null || /^bytes=0-/.test(range);
  return request.method === "GET" && response.status < 400 && fromStart;
}

function version(url: URL): string {
  return (
    url.searchParams.get("version") ?? url.pathname.match(/^\/releases\/(v[^/]+)\//)?.[1] ?? ""
  );
}

export interface Hit {
  kind: Kind;
  path: string;
  file: string;
  version: string;
  target: string;
  arch: string;
  country: string;
  status: number;
}

export function hit(request: Request, response: Response): Hit {
  const url = new URL(request.url);
  return {
    kind: kind(url.pathname),
    path: url.pathname,
    file: url.pathname.slice(url.pathname.lastIndexOf("/") + 1),
    version: version(url),
    target: url.searchParams.get("target") ?? "",
    arch: url.searchParams.get("arch") ?? "",
    country: (request.cf?.country as string | undefined) ?? "",
    status: response.status,
  };
}

export function point(entry: Hit): AnalyticsEngineDataPoint {
  return {
    indexes: [entry.kind],
    blobs: [
      entry.kind,
      entry.path,
      entry.version,
      entry.target,
      entry.arch,
      entry.country,
      String(entry.status),
    ],
    doubles: [1],
  };
}

export default {
  async fetch(request, env) {
    const origin = new URL(request.url);
    origin.search = "";
    const response = await fetch(new Request(origin, request));
    if (counted(request, response)) {
      const entry = hit(request, response);
      env.DOWNLOADS.writeDataPoint(point(entry));
      console.log({ message: "hit", ...entry });
    }
    return response;
  },
} satisfies ExportedHandler<Env>;
