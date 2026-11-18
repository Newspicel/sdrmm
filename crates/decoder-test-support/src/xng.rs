use xng_types::{AppInfo, Message, Provenance, StationIdentity};

use sdrmm_decoder::link::{self, DataLinkMessage, Fields, Quality};

/// Where xng's message bodies keep their envelope, across every mode it decodes.
pub const FIELDS: Fields = Fields::station(&[
    "from", "tail", "src", "mmsi", "icao", "mes_id", "gs_name", "sat",
])
.text(&["text", "message", "nature_description"]);

#[must_use]
pub fn provenance() -> Provenance {
    Provenance {
        station: StationIdentity::new("SDRmm"),
        app: AppInfo {
            name: "SDRmm".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        },
        sdr: None,
        channel: None,
    }
}

/// `message` as our data-link decode reads it.
#[must_use]
pub fn structured(message: &Message) -> DataLinkMessage {
    link::message(
        &message.body,
        &FIELDS,
        Quality {
            crc_ok: message.decode.crc_ok,
            fec_corrected: message.decode.fec_corrected,
            snr_db: message.signal.snr_db,
            frequency_error_hz: message.signal.freq_skew_hz,
        },
        message.raw.as_deref(),
    )
}
