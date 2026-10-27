import { describe, expect, it } from "vitest";
import type { DecodedRecordOf, LoraFrame } from "../../lib/types";
import { NOW, record } from "../../test/decoderRecords";
import { loraStations } from "./lora";

function meshtastic(
  from: number,
  content: NonNullable<Extract<LoraFrame["decoded"], { protocol: "meshtastic" }>["content"]>,
): LoraFrame["decoded"] {
  return {
    protocol: "meshtastic",
    to: 0xffff_ffff,
    from,
    id: 1,
    hop_limit: 3,
    hop_start: 3,
    want_ack: false,
    via_mqtt: false,
    channel_hash: 8,
    next_hop: 0,
    relay_node: 0,
    encryption: "channel",
    content,
  };
}

function loraAt(
  offsetS: number,
  decoded: LoraFrame["decoded"],
  snrDb = 0,
): DecodedRecordOf<"lora"> {
  return record(
    "lora",
    {
      spreading_factor: 11,
      bandwidth_hz: 250_000,
      coding_rate: "4/5",
      sync_word: 0x2b,
      implicit_header: false,
      low_data_rate: false,
      inverted_iq: false,
      integrity: "crc_ok",
      fec_corrected: 0,
      snr_db: snrDb,
      frequency_error_hz: 0,
      payload: "00",
      decoded,
    },
    { at: new Date(NOW + offsetS * 1000).toISOString(), freq_hz: 869_525_000 },
  );
}

describe("loraStations", () => {
  it("lists heard nodes newest first, keeping the last known name", () => {
    const stations = loraStations([
      loraAt(3, meshtastic(0xa1, { type: "text", text: "hi" }), -4),
      loraAt(2, {
        protocol: "lorawan",
        message_type: "unconfirmed_up",
        major: 0,
        mic: "00",
        dev_addr: "26011BDA",
      }),
      loraAt(
        1,
        meshtastic(0xa1, {
          type: "node_info",
          id: "!000000a1",
          long_name: "Base camp",
          short_name: "BC",
          licensed: false,
        }),
      ),
      loraAt(0, null),
    ]);
    expect(stations.map((s) => s.key)).toEqual(["!000000a1", "26011BDA"]);
    expect(stations[0]).toMatchObject({
      protocol: "Meshtastic",
      name: "Base camp",
      message: "!000000a1 → ^all · hi",
      snrDb: -4,
      frames: 2,
    });
    expect(stations[1]).toMatchObject({ protocol: "LoRaWAN", name: null, frames: 1 });
  });
});
