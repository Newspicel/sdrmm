export interface DocItem {
  slug: string;
  label: string;
  children?: DocItem[];
}

export interface DocSection {
  title: string;
  items: DocItem[];
}

export const DOCS_NAV: readonly DocSection[] = [
  {
    title: "Start",
    items: [
      { slug: "", label: "Overview" },
      { slug: "start/install", label: "Install" },
      { slug: "start/first-receiver", label: "Your first receiver" },
      { slug: "start/concepts", label: "Nodes and wires" },
      { slug: "start/workspaces", label: "Workspaces and presets" },
    ],
  },
  {
    title: "Radios",
    items: [
      { slug: "radios", label: "Supported radios" },
      { slug: "radios/usb", label: "USB radios" },
      { slug: "radios/network", label: "Network and virtual" },
    ],
  },
  {
    title: "Operate",
    items: [
      { slug: "operate/channels", label: "Channels and audio" },
      { slug: "operate/displays", label: "Displays and logs" },
      {
        slug: "operate/decoders",
        label: "Decoders",
        children: [
          { slug: "operate/decoders/voice", label: "Digital voice and trunking" },
          { slug: "operate/decoders/aviation-marine", label: "Aviation and marine" },
          { slug: "operate/decoders/amateur-hf", label: "Amateur and HF" },
          { slug: "operate/decoders/broadcast-video", label: "Broadcast and video" },
          { slug: "operate/decoders/weather-space", label: "Weather and satellites" },
          { slug: "operate/decoders/utility-iot", label: "Utility and IoT" },
        ],
      },
      { slug: "operate/finding-signals", label: "Finding signals" },
      { slug: "operate/recording", label: "Recording and playback" },
      { slug: "operate/transmit", label: "Transmit" },
      { slug: "operate/position-satellites", label: "Position and satellites" },
      { slug: "operate/rotators", label: "Rotators" },
      { slug: "operate/propagation", label: "Propagation map" },
      { slug: "operate/arrays", label: "Arrays" },
      { slug: "operate/direction-finding", label: "Direction finding" },
      { slug: "operate/passive-radar", label: "Passive radar" },
      { slug: "operate/network-export", label: "Network export" },
      { slug: "operate/tools", label: "Tools" },
      { slug: "operate/phones", label: "Phones" },
    ],
  },
  {
    title: "Server",
    items: [
      { slug: "server/configuration", label: "Configuration and security" },
      { slug: "server/deployment", label: "Deployment" },
      { slug: "server/tunnels", label: "HTTPS with a tunnel" },
      { slug: "server/api", label: "API and MCP" },
    ],
  },
  {
    title: "Reference",
    items: [
      { slug: "reference/wires", label: "Wires and what they carry" },
      { slug: "reference/keyboard", label: "Keyboard controls" },
      { slug: "reference/troubleshooting", label: "Troubleshooting" },
    ],
  },
  {
    title: "Develop",
    items: [
      { slug: "develop/building", label: "Build and test" },
      { slug: "develop/architecture", label: "Architecture" },
      {
        slug: "develop/plugins",
        label: "Plugins",
        children: [{ slug: "develop/plugin-abi", label: "Any language ABI" }],
      },
      { slug: "develop/releases", label: "Releases" },
      { slug: "develop/hardware-tests", label: "Hardware tests" },
      { slug: "develop/gpu-performance", label: "GPU measurements" },
      { slug: "develop/phone-apps", label: "Phone apps" },
    ],
  },
];

export interface FlatDocItem {
  slug: string;
  label: string;
  section: string;
  parent?: { slug: string; label: string };
}

export function flattenNav(nav: readonly DocSection[] = DOCS_NAV): FlatDocItem[] {
  const flat: FlatDocItem[] = [];
  for (const section of nav) {
    for (const item of section.items) {
      flat.push({ slug: item.slug, label: item.label, section: section.title });
      if (item.children) {
        for (const child of item.children) {
          flat.push({
            slug: child.slug,
            label: child.label,
            section: section.title,
            parent: { slug: item.slug, label: item.label },
          });
        }
      }
    }
  }
  return flat;
}

export const FLAT_DOCS: readonly FlatDocItem[] = flattenNav();

export function docHref(slug: string): string {
  return slug === "" ? "/docs/" : `/docs/${slug}`;
}

export function normalizeDocId(entry: string): string {
  const clean = entry.replace(/\\/g, "/").replace(/\.md$/, "");
  if (clean === "index") {
    return "";
  }
  if (clean.endsWith("/index")) {
    return clean.slice(0, -"/index".length);
  }
  return clean;
}

export function docSourcePath(slug: string, existingFiles: ReadonlySet<string>): string {
  if (slug === "") {
    return "docs/index.md";
  }
  const indexCandidate = `docs/${slug}/index.md`;
  if (existingFiles.has(indexCandidate)) {
    return indexCandidate;
  }
  return `docs/${slug}.md`;
}

export function prevNext(slug: string): { prev?: FlatDocItem; next?: FlatDocItem } {
  const index = FLAT_DOCS.findIndex((item) => item.slug === slug);
  if (index === -1) {
    return {};
  }
  return {
    prev: FLAT_DOCS[index - 1],
    next: FLAT_DOCS[index + 1],
  };
}

export function findDoc(slug: string): FlatDocItem | undefined {
  return FLAT_DOCS.find((item) => item.slug === slug);
}
