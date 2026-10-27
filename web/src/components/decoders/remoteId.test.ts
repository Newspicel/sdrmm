import { describe, expect, it } from "vitest";
import type { DecoderEventOf } from "../../lib/types";
import { record } from "../../test/decoderRecords";
import { remoteIdDrones } from "./remoteId";

function droneFrame(
  messages: DecoderEventOf<"remote_id">["data"]["messages"],
  transport: "bluetooth_legacy" | "wifi_beacon",
): DecoderEventOf<"remote_id">["data"] {
  return {
    transport,
    phy: "le1m",
    address: "D4:5A:21:0C:7E:19",
    uas_id: "1581F5FJD239C00DW22E",
    level_dbfs: -42,
    messages,
  };
}

describe("remoteIdDrones", () => {
  it("joins one drone's adverts into a row with its pilot distance", () => {
    const drones = remoteIdDrones([
      record(
        "remote_id",
        droneFrame(
          [
            {
              type: "location",
              status: "airborne",
              lat: 52.5163,
              lon: 13.3777,
              height_m: 84,
              speed_mps: 6.5,
              height_reference: "takeoff",
            },
          ],
          "bluetooth_legacy",
        ),
        { at: "2026-08-09T12:00:01Z" },
      ),
      record(
        "remote_id",
        droneFrame(
          [
            {
              type: "basic_id",
              id_type: "serial_number",
              ua_type: "rotorcraft",
              uas_id: "1581F5FJD239C00DW22E",
            },
            {
              type: "system",
              operator_location_type: "live_gnss",
              operator_lat: 52.5156,
              operator_lon: 13.3761,
              area_count: 1,
              area_radius_m: 0,
            },
          ],
          "wifi_beacon",
        ),
        { at: "2026-08-09T12:00:02Z" },
      ),
    ]);
    expect(drones).toHaveLength(1);
    const drone = drones[0];
    if (drone === undefined) {
      throw new Error("no drone");
    }
    expect(drone.key).toBe("1581F5FJD239C00DW22E");
    expect(drone.kind).toBe("Multirotor");
    expect(drone.heightM).toBe(84);
    expect(drone.links).toEqual(["BT4", "Wi-Fi beacon"]);
    expect(drone.frames).toBe(2);
    expect(Math.round(drone.pilotM ?? 0)).toBe(133);
  });
});
