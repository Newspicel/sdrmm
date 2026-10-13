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
