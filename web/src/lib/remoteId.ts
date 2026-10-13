import type {
  RemoteIdFrame,
  RemoteIdLocation,
  RemoteIdMessage,
  RemoteIdSystem,
  UaType,
} from "./types";

export const REMOTE_ID_TRANSPORT_LABELS: Record<RemoteIdFrame["transport"], string> = {
  bluetooth_legacy: "BT4",
  bluetooth_extended: "BT5",
  wifi_beacon: "Wi-Fi beacon",
  wifi_beacon_french: "Wi-Fi FR",
  wifi_beacon_dji: "DJI",
  wifi_nan: "Wi-Fi NAN",
};

export const UA_TYPE_LABELS: Record<UaType, string> = {
  none: "-",
  aeroplane: "Aeroplane",
  rotorcraft: "Multirotor",
  gyroplane: "Gyroplane",
  hybrid_lift: "VTOL",
  ornithopter: "Ornithopter",
  glider: "Glider",
  kite: "Kite",
  free_balloon: "Balloon",
  captive_balloon: "Captive balloon",
  airship: "Airship",
  parachute: "Parachute",
  rocket: "Rocket",
  tethered_aircraft: "Tethered",
  ground_obstacle: "Obstacle",
  other: "Other",
};

export interface LatLon {
  lat: number;
  lon: number;
}

export function remoteIdStation(frame: RemoteIdFrame): string {
  return frame.uas_id ?? frame.address;
}

export function remoteIdLocation(frame: RemoteIdFrame): RemoteIdLocation | null {
  for (const message of frame.messages) {
    if (message.type === "location") {
      return message;
    }
  }
  return null;
}

export function remoteIdSystem(frame: RemoteIdFrame): RemoteIdSystem | null {
  for (const message of frame.messages) {
    if (message.type === "system") {
      return message;
    }
  }
  return null;
}

export function remoteIdUaType(frame: RemoteIdFrame): UaType | null {
  for (const message of frame.messages) {
    if (message.type === "basic_id") {
      return message.ua_type;
    }
  }
  return null;
}

export function remoteIdOperator(frame: RemoteIdFrame): string | null {
  for (const message of frame.messages) {
    if (message.type === "operator_id" && message.operator_id !== "") {
      return message.operator_id;
    }
  }
  return null;
}

export function remoteIdText(frame: RemoteIdFrame): string | null {
  for (const message of frame.messages) {
    if (message.type === "self_id" && message.text !== "") {
      return message.text;
    }
  }
  return null;
}

function point(lat: number | null | undefined, lon: number | null | undefined): LatLon | null {
  return lat == null || lon == null ? null : { lat, lon };
}

export function remoteIdPosition(frame: RemoteIdFrame): LatLon | null {
  const location = remoteIdLocation(frame);
  return point(location?.lat, location?.lon);
}

export function remoteIdPilot(frame: RemoteIdFrame): LatLon | null {
  const system = remoteIdSystem(frame);
  return point(system?.operator_lat, system?.operator_lon);
}

function slot(message: RemoteIdMessage): string {
  switch (message.type) {
    case "basic_id":
      return `basic_id:${message.id_type}`;
    case "authentication":
      return `authentication:${message.page}`;
    case "unknown":
      return `unknown:${message.message_type}`;
    default:
      return message.type;
  }
}

export function withRemoteIdState(previous: RemoteIdFrame, next: RemoteIdFrame): RemoteIdFrame {
  const fresh = new Set(next.messages.map(slot));
  const kept = previous.messages.filter((message) => !fresh.has(slot(message)));
  return {
    ...next,
    uas_id: next.uas_id ?? previous.uas_id ?? null,
    ssid: next.ssid ?? previous.ssid ?? null,
    messages: [...next.messages, ...kept],
  };
}

export function distanceM(a: LatLon, b: LatLon): number {
  const radius = 6_371_000;
  const rad = Math.PI / 180;
  const dLat = (b.lat - a.lat) * rad;
  const dLon = (b.lon - a.lon) * rad;
  const h =
    Math.sin(dLat / 2) ** 2 +
    Math.cos(a.lat * rad) * Math.cos(b.lat * rad) * Math.sin(dLon / 2) ** 2;
  return 2 * radius * Math.asin(Math.sqrt(h));
}
