const NOT_FOUND = "/404";
const DOCS = "/docs";

const LEGACY_DOCS: Record<string, string> = {
  "/docs/getting-started/install": "/docs/start/install",
  "/docs/getting-started/first-receiver": "/docs/start/first-receiver",
  "/docs/getting-started/workspace": "/docs/start/concepts",
  "/docs/wires": "/docs/reference/wires",
  "/docs/hardware": "/docs/radios",
  "/docs/troubleshooting": "/docs/reference/troubleshooting",
  "/docs/user-guide/workspaces": "/docs/start/workspaces",
  "/docs/user-guide/channels": "/docs/operate/channels",
  "/docs/user-guide/decoders": "/docs/operate/decoders",
  "/docs/user-guide/scanning": "/docs/operate/finding-signals",
  "/docs/user-guide/recording": "/docs/operate/recording",
  "/docs/user-guide/network-iq": "/docs/operate/network-export",
  "/docs/user-guide/transmit": "/docs/operate/transmit",
  "/docs/user-guide/position": "/docs/operate/position-satellites",
  "/docs/user-guide/satellites": "/docs/operate/position-satellites",
  "/docs/user-guide/rotators": "/docs/operate/rotators",
  "/docs/user-guide/propagation": "/docs/operate/propagation",
  "/docs/user-guide/arrays": "/docs/operate/arrays",
  "/docs/user-guide/direction-finding": "/docs/operate/direction-finding",
  "/docs/user-guide/passive-radar": "/docs/operate/passive-radar",
  "/docs/user-guide/phones": "/docs/operate/phones",
  "/docs/user-guide/tools": "/docs/operate/tools",
  "/docs/user-guide/keyboard": "/docs/reference/keyboard",
  "/docs/development/building": "/docs/develop/building",
  "/docs/development/architecture": "/docs/develop/architecture",
  "/docs/development/plugins": "/docs/develop/plugins",
  "/docs/development/plugin-abi": "/docs/develop/plugin-abi",
  "/docs/development/phone-apps": "/docs/develop/phone-apps",
  "/docs/development/hardware-tests": "/docs/develop/hardware-tests",
  "/docs/development/gpu-performance": "/docs/develop/gpu-performance",
  "/docs/development/releases": "/docs/develop/releases",
};

export function docsPath(pathname: string): string | null {
  const page = pathname.replace(/\/+$/, "").replace(/\.html$/, "");
  if (page in LEGACY_DOCS) {
    return LEGACY_DOCS[page] ?? null;
  }
  const inDocs = page === DOCS || page.startsWith(`${DOCS}/`);
  if (page === "" || inDocs || /\.[^/]*$/.test(page)) {
    return null;
  }
  const candidate = `${DOCS}${page}`;
  return LEGACY_DOCS[candidate] ?? candidate;
}

async function movedToDocs(url: URL, env: Env): Promise<URL | null> {
  const path = docsPath(url.pathname);
  if (path === null) {
    return null;
  }
  const target = new URL(path, url);
  target.search = url.search;
  const found = await env.ASSETS.fetch(target, { method: "HEAD" });
  return found.ok ? target : null;
}

export async function missing(request: Request, env: Env): Promise<Response> {
  const url = new URL(request.url);
  const target = await movedToDocs(url, env);
  if (target !== null) {
    return Response.redirect(target.href, 301);
  }
  const notFound = await env.ASSETS.fetch(new URL(NOT_FOUND, url));
  return new Response(notFound.body, { status: 404, headers: notFound.headers });
}
