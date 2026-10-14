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

export function point(request: Request, response: Response): AnalyticsEngineDataPoint {
  const url = new URL(request.url);
  const country = (request.cf?.country as string | undefined) ?? "";
  const kindOf = kind(url.pathname);
  return {
    indexes: [kindOf],
    blobs: [
      kindOf,
      url.pathname,
      version(url),
      url.searchParams.get("target") ?? "",
      url.searchParams.get("arch") ?? "",
      country,
      String(response.status),
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
      env.DOWNLOADS.writeDataPoint(point(request, response));
    }
    return response;
  },
} satisfies ExportedHandler<Env>;
