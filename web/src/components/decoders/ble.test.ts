import { describe, expect, it } from "vitest";
import { record } from "../../test/decoderRecords";
import { bleDevices } from "./ble";

function anonymousAdvert(data_id: number) {
  return record("ble", {
    pdu: "adv_ext_ind",
    phy: "le1m",
    channel: 37,
    level_dbfs: -60,
    adi: { set: 0, data_id },
    data: "",
  });
}

describe("bleDevices", () => {
  it("joins a device's advert and scan response into one row", () => {
    const address = { address: "C0:11:22:33:44:55", kind: "random_static" as const };
    const devices = bleDevices([
      record(
        "ble",
        {
          pdu: "adv_ind",
          phy: "le1m",
          channel: 38,
          address,
          level_dbfs: -50,
          manufacturer: [{ company_id: 76, company: "Apple", data: "1005" }],
          data: "",
        },
        { at: "2026-08-09T12:00:01Z" },
      ),
      record(
        "ble",
        {
          pdu: "scan_rsp",
          phy: "le1m",
          channel: 38,
          address,
          level_dbfs: -48,
          repeats: 4,
          name: "Thermo",
          data: "",
        },
        { at: "2026-08-09T12:00:02Z" },
      ),
    ]);
    expect(devices).toHaveLength(1);
    expect(devices[0]).toMatchObject({
      key: "C0:11:22:33:44:55",
      name: "Thermo",
      vendor: "Apple",
      kind: "SCAN_RSP",
      levelDbfs: -48,
      heard: 6,
    });
  });

  it("keeps anonymous extended advertisers apart by their data ID", () => {
    const devices = bleDevices([anonymousAdvert(0xe75), anonymousAdvert(0x31a)]);
    expect(devices.map((device) => device.key).toSorted()).toEqual([
      "ADV_EXT_IND SID 0 DID 31A",
      "ADV_EXT_IND SID 0 DID E75",
    ]);
  });
});
