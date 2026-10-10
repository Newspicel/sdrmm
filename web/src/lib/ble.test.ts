import { describe, expect, it } from "vitest";
import { bleBeaconText, bleSummary } from "./ble";

describe("ble", () => {
  it("summarises an advert by address, name and vendor", () => {
    expect(
      bleSummary({
        pdu: "adv_ind",
        phy: "le1m",
        address: { address: "C0:11:22:33:44:55", kind: "random_static" },
        level_dbfs: -41.4,
        name: "Thermo",
        manufacturer: [{ company_id: 76, company: "Apple", data: "1005" }],
        data: "",
      }),
    ).toBe("C0:11:22:33:44:55 · Thermo · Apple · -41 dBFS");
    expect(bleSummary({ pdu: "adv_ext_ind", phy: "le1m", level_dbfs: -60, data: "" })).toBe(
      "ADV_EXT_IND · -60 dBFS",
    );
  });

  it("reads beacons as text", () => {
    expect(
      bleBeaconText({ type: "ibeacon", uuid: "U", major: 7, minor: 300, measured_dbm: -59 }),
    ).toBe("U 7/300 · -59 dBm at 1 m");
    expect(
      bleBeaconText({
        type: "eddystone_tlm",
        battery_mv: 3000,
        temperature_c: 23.5,
        adverts: 5,
        uptime_s: 12.4,
      }),
    ).toBe("3.00 V · 23.5 °C · 5 adverts · up 12 s");
  });
});
