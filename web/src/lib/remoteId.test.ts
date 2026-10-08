import { describe, expect, it } from "vitest";
import {
  distanceM,
  remoteIdPilot,
  remoteIdPosition,
  remoteIdStation,
  remoteIdSummary,
  withRemoteIdState,
} from "./remoteId";
import type { RemoteIdFrame, RemoteIdMessage } from "./types";

const LOCATION: RemoteIdMessage = {
  type: "location",
  status: "airborne",
  lat: 52.5163,
  lon: 13.3777,
  height_m: 84,
  height_reference: "takeoff",
  speed_mps: 6.5,
  track_deg: 74,
};

const BASIC: RemoteIdMessage = {
  type: "basic_id",
  id_type: "serial_number",
  ua_type: "rotorcraft",
  uas_id: "1581F5FJD239C00DW22E",
};

const SYSTEM: RemoteIdMessage = {
  type: "system",
  operator_location_type: "live_gnss",
  operator_lat: 52.5156,
  operator_lon: 13.3761,
  area_count: 1,
  area_radius_m: 0,
};

function droneFrame(messages: RemoteIdMessage[], over: Partial<RemoteIdFrame> = {}): RemoteIdFrame {
  return {
    transport: "bluetooth_legacy",
    phy: "le1m",
    address: "D4:5A:21:0C:7E:19",
    level_dbfs: -42,
    messages,
    ...over,
  };
}

describe("remote ID frames", () => {
  it("name the drone by its serial, else by its address", () => {
    expect(remoteIdStation(droneFrame([]))).toBe("D4:5A:21:0C:7E:19");
    expect(remoteIdStation(droneFrame([], { uas_id: "1581F5FJD239C00DW22E" }))).toBe(
      "1581F5FJD239C00DW22E",
    );
  });

  it("keep earlier messages until a newer one of the same kind arrives", () => {
    const first = droneFrame([BASIC, SYSTEM], { uas_id: "1581F5FJD239C00DW22E" });
    const merged = withRemoteIdState(first, droneFrame([LOCATION]));
    expect(merged.uas_id).toBe("1581F5FJD239C00DW22E");
    expect(merged.messages.map((message) => message.type)).toEqual([
      "location",
      "basic_id",
      "system",
    ]);
    expect(remoteIdPosition(merged)).toEqual({ lat: 52.5163, lon: 13.3777 });
    expect(remoteIdPilot(merged)).toEqual({ lat: 52.5156, lon: 13.3761 });
  });

  it("summarise like the server does", () => {
    const frame = droneFrame([LOCATION, SYSTEM], { uas_id: "1581F5FJD239C00DW22E" });
    expect(remoteIdSummary(frame)).toBe(
      "1581F5FJD239C00DW22E · BT4 · 52.51630, 13.37770 · 84 m · pilot 52.51560, 13.37610",
    );
  });

  it("measure the drone to pilot distance", () => {
    const meters = distanceM({ lat: 52.5163, lon: 13.3777 }, { lat: 52.5156, lon: 13.3761 });
    expect(meters).toBeGreaterThan(125);
    expect(meters).toBeLessThan(135);
  });
});
