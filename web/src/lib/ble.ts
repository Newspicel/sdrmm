import type { BleAdvert, BleBeacon } from "./types";

export const BLE_PDU_LABELS: Record<BleAdvert["pdu"], string> = {
  adv_ind: "ADV_IND",
  adv_direct_ind: "ADV_DIRECT_IND",
  adv_nonconn_ind: "ADV_NONCONN_IND",
  scan_req: "SCAN_REQ",
  scan_rsp: "SCAN_RSP",
  connect_ind: "CONNECT_IND",
  adv_scan_ind: "ADV_SCAN_IND",
  adv_ext_ind: "ADV_EXT_IND",
  aux_adv_ind: "AUX_ADV_IND",
  aux_connect_rsp: "AUX_CONNECT_RSP",
};

export const BLE_PHY_LABELS: Record<BleAdvert["phy"], string> = {
  le1m: "LE 1M",
  le2m: "LE 2M",
  le_coded_s8: "LE Coded S8",
  le_coded_s2: "LE Coded S2",
  le_coded: "LE Coded",
};

export const BLE_ADDRESS_KIND_LABELS: Record<NonNullable<BleAdvert["address"]>["kind"], string> = {
  public: "public",
  random_static: "random static",
  resolvable_private: "resolvable private",
  non_resolvable_private: "non-resolvable private",
  reserved: "reserved",
};

export const BLE_BEACON_LABELS: Record<BleBeacon["type"], string> = {
  ibeacon: "iBeacon",
  alt_beacon: "AltBeacon",
  eddystone_uid: "Eddystone UID",
  eddystone_url: "Eddystone URL",
  eddystone_tlm: "Eddystone TLM",
  eddystone_eid: "Eddystone EID",
  find_my: "Find My",
  exposure_notification: "Exposure Notification",
  fast_pair: "Fast Pair",
};

export function bleStation(advert: BleAdvert): string | null {
  return advert.address?.address ?? null;
}

export function bleVendor(advert: BleAdvert): string | null {
  return advert.manufacturer?.find((maker) => maker.company != null)?.company ?? null;
}

export function bleBeaconText(beacon: BleBeacon): string {
  switch (beacon.type) {
    case "ibeacon":
      return `${beacon.uuid} ${beacon.major}/${beacon.minor} · ${beacon.measured_dbm} dBm at 1 m`;
    case "alt_beacon":
      return `${beacon.id} · ${beacon.measured_dbm} dBm at 1 m`;
    case "eddystone_uid":
      return `${beacon.namespace} ${beacon.instance}`;
    case "eddystone_url":
      return beacon.url;
    case "eddystone_tlm":
      return [
        beacon.battery_mv == null ? null : `${(beacon.battery_mv / 1000).toFixed(2)} V`,
        beacon.temperature_c == null ? null : `${beacon.temperature_c.toFixed(1)} °C`,
        `${beacon.adverts} adverts`,
        `up ${Math.round(beacon.uptime_s)} s`,
      ]
        .filter((part) => part !== null)
        .join(" · ");
    case "eddystone_eid":
      return beacon.eid;
    case "find_my":
      return beacon.maintained ? "owner nearby" : "separated";
    case "exposure_notification":
      return beacon.identifier;
    case "fast_pair":
      return `model ${beacon.model}`;
  }
}

export function bleSummary(advert: BleAdvert): string {
  const parts = [
    bleStation(advert),
    advert.name ?? null,
    advert.beacon == null ? null : BLE_BEACON_LABELS[advert.beacon.type],
    bleVendor(advert),
  ].filter((part) => part !== null);
  if (parts.length === 0) {
    parts.push(BLE_PDU_LABELS[advert.pdu]);
  }
  parts.push(`${advert.level_dbfs.toFixed(0)} dBFS`);
  return parts.join(" · ");
}
