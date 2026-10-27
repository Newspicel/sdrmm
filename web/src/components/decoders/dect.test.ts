import { describe, expect, it } from "vitest";
import { record } from "../../test/decoderRecords";
import { dectStations } from "./dect";

function dectFrame(side: "rfp" | "pp", playing: boolean | null) {
  return {
    side,
    update: "voice" as const,
    identity: {
      rfpi: "01234D5E6D",
      pari: "02469ABCD",
      arc: "a" as const,
      sari_available: false,
      rpn: 5,
    },
    security: {
      cipher_state: "clear" as const,
      encryption_events: 0,
      authentication_supported: true,
      dsaa2_supported: true,
    },
    voice:
      playing === null
        ? undefined
        : { frames: 10, playing, x_crc_errors: 0, unsynced: 0, encrypted: 0, late: 0 },
    extended_carriers: false,
    bursts: 10,
    level_dbfs: side === "rfp" ? -30 : -60,
  };
}

describe("DECT stations", () => {
  it("keeps the base's row and takes only voice from the handset", () => {
    const [base] = dectStations([
      record("dect", dectFrame("rfp", null), { at: "2026-08-09T12:00:00Z" }),
      record("dect", dectFrame("pp", true), { at: "2026-08-09T12:00:01Z" }),
    ]);
    expect(base?.levelDbfs).toBe(-30);
    expect(base?.voice).toBe("playing");
    expect(base?.dsaa2).toBe(true);
    expect(base?.dsc2).toBeNull();
  });
});
