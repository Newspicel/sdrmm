import type { SarsatBeacon, SarsatMessage, SarsatProtocol } from "./types";

export const SARSAT_BEACON_LABELS: Record<SarsatBeacon, string> = {
  epirb: "EPIRB",
  elt: "ELT",
  plb: "PLB",
  ship_security: "SSAS",
  test: "Test",
  other: "Beacon",
};

export const SARSAT_PROTOCOL_LABELS: Record<SarsatProtocol, string> = {
  maritime_user: "Maritime user",
  radio_call_sign_user: "Call sign user",
  aviation_user: "Aviation user",
  serial_user: "Serial user",
  national_user: "National user",
  test_user: "Test user",
  orbitography: "Orbitography",
  second_generation: "Second generation",
  standard_location: "Standard location",
  ship_security: "Ship security",
  national_location: "National location",
  rls_location: "RLS location",
  elt_dt_location: "ELT(DT) location",
  standard_test: "Standard test",
  national_test: "National test",
  spare: "Spare",
};

export function sarsatName(message: SarsatMessage): string {
  return `${SARSAT_BEACON_LABELS[message.beacon]} ${message.hex_id}`;
}

export function sarsatChecks(message: SarsatMessage): string {
  if (message.pdf2_ok === false) {
    return "PDF-2 failed";
  }
  return message.long ? "both fields ok" : "PDF-1 ok";
}
