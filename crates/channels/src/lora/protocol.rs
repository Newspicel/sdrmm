use sdrmm_wire::{LoraKey, LoraPayload, LoraProtocol};

use super::{lorawan, meshcore, meshtastic};

pub(crate) struct Keyring {
    lorawan: lorawan::Keys,
    meshtastic: meshtastic::Keys,
    meshcore: meshcore::Keys,
}

impl Keyring {
    pub(crate) fn new(keys: &[LoraKey]) -> Result<Self, String> {
        Ok(Self {
            lorawan: lorawan::Keys::new(keys)?,
            meshtastic: meshtastic::Keys::new(keys)?,
            meshcore: meshcore::Keys::new(keys)?,
        })
    }
}

fn lorawan(payload: &[u8], keys: &Keyring) -> Option<LoraPayload> {
    lorawan::decode(payload, &keys.lorawan).map(LoraPayload::Lorawan)
}

fn meshtastic(payload: &[u8], keys: &Keyring) -> Option<LoraPayload> {
    meshtastic::decode(payload, &keys.meshtastic).map(LoraPayload::Meshtastic)
}

fn meshcore(payload: &[u8], keys: &Keyring) -> Option<LoraPayload> {
    meshcore::decode(payload, &keys.meshcore).map(LoraPayload::Meshcore)
}

pub(crate) fn interpret(
    protocol: LoraProtocol,
    sync_word: u8,
    payload: &[u8],
    keys: &Keyring,
) -> Option<LoraPayload> {
    match protocol {
        LoraProtocol::Raw => None,
        LoraProtocol::Lorawan => lorawan(payload, keys),
        LoraProtocol::Meshtastic => meshtastic(payload, keys),
        LoraProtocol::Meshcore => meshcore(payload, keys),
        LoraProtocol::Auto => match sync_word {
            LoraProtocol::LORAWAN_SYNC_WORD => lorawan(payload, keys),
            LoraProtocol::MESHTASTIC_SYNC_WORD => meshtastic(payload, keys),
            LoraProtocol::PRIVATE_SYNC_WORD => {
                meshcore(payload, keys).or_else(|| lorawan(payload, keys))
            }
            _ => None,
        },
    }
}
