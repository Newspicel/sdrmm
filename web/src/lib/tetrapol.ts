import type {
  TetrapolAddress,
  TetrapolCallType,
  TetrapolCell,
  TetrapolGroupCall,
  TetrapolLogicalChannel,
  TetrapolMessage,
  TetrapolPayload,
} from "./types";

export const TETRAPOL_CHANNEL_LABELS: Record<TetrapolLogicalChannel, string> = {
  broadcast: "BCH",
  paging: "PCH",
  signalling: "SDCH",
  traffic: "TCH",
  unknown: "CH",
};

export const TETRAPOL_CALL_LABELS: Record<TetrapolCallType, string> = {
  open_channel: "Open channel",
  internal: "Internal",
  broadcast: "Broadcast",
  external: "External",
};

export function tetrapolAddress(address: TetrapolAddress): string {
  switch (address.kind) {
    case "terminal":
      return `TTI ${address.id.toString(16).toUpperCase().padStart(4, "0")}`;
    case "random":
      return `RTI ${address.id}`;
    case "group":
      return `CGI ${address.id}`;
    case "object":
      return `COI ${address.id}`;
    case "all_stations":
      return "all";
  }
}

export function tetrapolCell(cell: TetrapolCell): string {
  return `${cell.bn}.${cell.rsw}.${cell.bs}`;
}

export function tetrapolEncrypted(call: TetrapolGroupCall): boolean {
  return call.key.index !== 0 || (call.key.kind !== 0 && call.key.kind !== 15);
}

function callTail(call: TetrapolGroupCall): string {
  const channel = call.channel == null ? "" : ` on channel ${call.channel}`;
  return `${channel}${tetrapolEncrypted(call) ? ", encrypted" : ""}`;
}

function payloadSummary(payload: TetrapolPayload): string {
  switch (payload.message) {
    case "system_info":
      return [
        `cell ${tetrapolCell(payload.cell)}`,
        payload.country == null ? null : `country ${payload.country}`,
        payload.network == null ? null : `network ${payload.network}`,
        payload.ciphered ? "ciphered" : null,
      ]
        .filter((part) => part != null)
        .join(" · ");
    case "group_activation":
      return `group ${payload.group} active${callTail(payload)}`;
    case "group_paging":
      return `group ${payload.group} paged${callTail(payload)}`;
    case "group_idle":
      return "group idle";
    case "group_end":
      return `group end, cause 0x${payload.cause.toString(16).padStart(2, "0")}`;
    case "paging":
      return `paging ${payload.terminals.map(tetrapolAddress).join(", ")}`;
    case "voice":
      return `voice ${payload.seconds.toFixed(1)} s`;
    case "other":
      return payload.name;
  }
}

export function tetrapolSummary(message: TetrapolMessage): string {
  return [
    TETRAPOL_CHANNEL_LABELS[message.channel],
    message.address == null ? null : tetrapolAddress(message.address),
    payloadSummary(message.payload),
  ]
    .filter((part) => part != null)
    .join(" · ");
}

export function tetrapolStation(message: TetrapolMessage): string | null {
  const payload = message.payload;
  switch (payload.message) {
    case "system_info":
      return tetrapolCell(payload.cell);
    case "group_activation":
    case "group_paging":
      return `group ${payload.group}`;
    default:
      return message.address == null ? null : tetrapolAddress(message.address);
  }
}
