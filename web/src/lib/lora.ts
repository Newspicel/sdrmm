import { formatHz } from "../components/format";
import type {
  LoraFrame,
  LorawanFrame,
  MeshcorePacket,
  MeshtasticContent,
  MeshtasticPacket,
} from "./types";

export const MESHTASTIC_BROADCAST = 0xffff_ffff;

type Protocol = NonNullable<LoraFrame["decoded"]>["protocol"];

export const LORA_PROTOCOL_LABELS: Record<Protocol, string> = {
  lorawan: "LoRaWAN",
  meshtastic: "Meshtastic",
  meshcore: "MeshCore",
};

export const LORA_INTEGRITY_LABELS: Record<LoraFrame["integrity"], string> = {
  crc_ok: "CRC ok",
  crc_failed: "CRC failed",
  no_crc: "no CRC",
  header_failed: "header failed",
};

export const LORAWAN_TYPE_LABELS: Record<LorawanFrame["message_type"], string> = {
  join_request: "Join request",
  join_accept: "Join accept",
  unconfirmed_up: "Uplink",
  unconfirmed_down: "Downlink",
  confirmed_up: "Confirmed uplink",
  confirmed_down: "Confirmed downlink",
  rejoin_request: "Rejoin request",
  proprietary: "Proprietary",
};

export const MESHCORE_TYPE_LABELS: Record<MeshcorePacket["payload_type"], string> = {
  request: "Request",
  response: "Response",
  text: "Text",
  ack: "Ack",
  advert: "Advert",
  group_text: "Group text",
  group_data: "Group data",
  anon_request: "Anon request",
  path: "Path",
  trace: "Trace",
  multipart: "Multipart",
  control: "Control",
  reserved: "Reserved",
  raw_custom: "Custom",
};

export function meshtasticNode(node: number): string {
  return node === MESHTASTIC_BROADCAST ? "^all" : `!${(node >>> 0).toString(16).padStart(8, "0")}`;
}

export function loraProtocol(frame: LoraFrame): string {
  return frame.decoded == null ? "LoRa" : LORA_PROTOCOL_LABELS[frame.decoded.protocol];
}

export function loraStation(frame: LoraFrame): string | null {
  const decoded = frame.decoded;
  if (decoded == null) {
    return null;
  }
  switch (decoded.protocol) {
    case "lorawan":
      return decoded.dev_addr ?? decoded.dev_eui ?? null;
    case "meshtastic":
      return meshtasticNode(decoded.from);
    case "meshcore": {
      const content = decoded.content;
      switch (content.type) {
        case "advert":
          return content.name ?? content.public_key.slice(0, 8);
        case "group_text":
          return content.sender ?? null;
        case "encrypted":
          return content.source;
        default:
          return null;
      }
    }
  }
}

export function loraPosition(frame: LoraFrame): { lat: number; lon: number } | null {
  const decoded = frame.decoded;
  if (decoded == null || decoded.protocol === "lorawan") {
    return null;
  }
  const content = decoded.content;
  if (content == null) {
    return null;
  }
  const located =
    content.type === "position" ||
    content.type === "waypoint" ||
    content.type === "map_report" ||
    content.type === "advert";
  if (!located || content.lat == null || content.lon == null) {
    return null;
  }
  return { lat: content.lat, lon: content.lon };
}

export function loraName(frame: LoraFrame): string | null {
  const decoded = frame.decoded;
  if (decoded == null || decoded.protocol === "lorawan") {
    return null;
  }
  const content = decoded.content;
  switch (content?.type) {
    case "node_info":
    case "map_report":
      return content.long_name.trim() || null;
    case "advert":
      return content.name ?? null;
    default:
      return null;
  }
}

export function withLoraPosition(previous: LoraFrame, next: LoraFrame): LoraFrame {
  if (loraPosition(next) !== null || loraPosition(previous) === null) {
    return next;
  }
  return { ...next, decoded: previous.decoded };
}

export function loraSummary(frame: LoraFrame): string {
  const parts = [`SF${frame.spreading_factor} ${formatHz(frame.bandwidth_hz)}`, loraMessage(frame)];
  if (frame.integrity !== "crc_ok") {
    parts.push(LORA_INTEGRITY_LABELS[frame.integrity]);
  }
  return parts.join(" · ");
}

export function loraMessage(frame: LoraFrame): string {
  const decoded = frame.decoded;
  if (decoded == null) {
    return `${Math.floor(frame.payload.length / 2)} bytes`;
  }
  switch (decoded.protocol) {
    case "lorawan":
      return lorawanSummary(decoded).join(" · ");
    case "meshtastic":
      return meshtasticSummary(decoded).join(" · ");
    case "meshcore":
      return meshcoreSummary(decoded).join(" · ");
  }
}

function lorawanSummary(frame: LorawanFrame): string[] {
  return [
    LORAWAN_TYPE_LABELS[frame.message_type],
    frame.dev_addr ?? null,
    frame.dev_eui == null ? null : `DevEUI ${frame.dev_eui}`,
    frame.f_cnt == null ? null : `FCnt ${frame.f_cnt}`,
    frame.f_port == null ? null : `port ${frame.f_port}`,
    (frame.mac_commands ?? []).join(", ") || null,
  ].filter((part) => part !== null);
}

function meshtasticSummary(packet: MeshtasticPacket): string[] {
  const route = `${meshtasticNode(packet.from)} → ${meshtasticNode(packet.to)}`;
  if (packet.content != null) {
    return [route, meshtasticContentSummary(packet.content)];
  }
  return [route, packet.encryption === "pki" ? "direct message" : "encrypted"];
}

function meshtasticContentSummary(content: MeshtasticContent): string {
  switch (content.type) {
    case "text":
      return content.text;
    case "position":
      return content.lat == null || content.lon == null
        ? "position"
        : `position ${content.lat.toFixed(5)}, ${content.lon.toFixed(5)}`;
    case "node_info":
      return `${content.long_name} (${content.short_name})`;
    case "telemetry":
      return `${content.kind} telemetry ${content.metrics
        .slice(0, 3)
        .map((metric) => `${metric.name} ${metric.value.toFixed(1)}`)
        .join(", ")}`;
    case "routing":
      return content.error == null ? "ack" : `routing ${content.error}`;
    case "traceroute":
      return `traceroute ${content.route.length} hops`;
    case "neighbor_info":
      return `${content.neighbors.length} neighbors`;
    case "waypoint":
      return `waypoint ${content.name}`;
    case "map_report":
      return `map report ${content.long_name}`;
    case "data":
      return `${Math.floor(content.bytes.length / 2)} bytes`;
  }
}

function meshcoreSummary(packet: MeshcorePacket): string[] {
  const parts = [MESHCORE_TYPE_LABELS[packet.payload_type]];
  const content = packet.content;
  if (content.type === "advert" && content.name != null) {
    parts.push(content.name);
  } else if (content.type === "group_text") {
    if (content.channel != null) {
      parts.push(content.channel);
    }
    parts.push(groupText(content.sender, content.text));
  } else if (content.type === "encrypted") {
    parts.push(`${content.source} → ${content.destination}`);
  }
  const hops = (packet.path ?? []).length;
  if (hops > 0) {
    parts.push(`${hops} hops`);
  }
  return parts;
}

function groupText(sender: string | null | undefined, text: string | null | undefined): string {
  if (text == null) {
    return "encrypted";
  }
  return sender == null ? text : `${sender}: ${text}`;
}
