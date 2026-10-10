use std::collections::VecDeque;

use sdrmm_wire::{RemoteIdFrame, RemoteIdMessage, RemoteIdPhy, RemoteIdTransport};

use super::odid;

const REMEMBERED: usize = 256;

pub(crate) struct Heard<'a> {
    pub transport: RemoteIdTransport,
    pub phy: RemoteIdPhy,
    pub address: String,
    pub channel: Option<u8>,
    pub counter: Option<u8>,
    pub ssid: Option<String>,
    pub level_dbfs: f32,
    pub payload: &'a [u8],
}

#[derive(Default)]
pub(crate) struct Tracker {
    ids: VecDeque<(String, String)>,
    rejected: u32,
}

impl Tracker {
    pub(crate) fn reject(&mut self, count: u32) {
        self.rejected = self.rejected.saturating_add(count);
    }

    pub(crate) fn frame(&mut self, heard: Heard<'_>) -> Option<RemoteIdFrame> {
        let Ok(messages) = odid::parse(heard.payload) else {
            self.rejected = self.rejected.saturating_add(1);
            return None;
        };
        let uas_id = match odid::uas_id(&messages) {
            Some(id) => {
                self.remember(&heard.address, &id);
                Some(id)
            }
            None => self.recall(&heard.address),
        };
        Some(RemoteIdFrame {
            transport: heard.transport,
            phy: heard.phy,
            address: heard.address,
            channel: heard.channel,
            counter: heard.counter,
            uas_id,
            ssid: heard.ssid,
            level_dbfs: heard.level_dbfs,
            messages,
            rejected: self.rejected,
        })
    }

    pub(crate) fn direct(
        &mut self,
        heard: Heard<'_>,
        messages: Vec<RemoteIdMessage>,
    ) -> RemoteIdFrame {
        let uas_id = odid::uas_id(&messages);
        if let Some(id) = &uas_id {
            self.remember(&heard.address, id);
        }
        RemoteIdFrame {
            transport: heard.transport,
            phy: heard.phy,
            address: heard.address,
            channel: heard.channel,
            counter: heard.counter,
            uas_id,
            ssid: heard.ssid,
            level_dbfs: heard.level_dbfs,
            messages,
            rejected: self.rejected,
        }
    }

    fn remember(&mut self, address: &str, id: &str) {
        if let Some(index) = self.ids.iter().position(|(known, _)| known == address) {
            self.ids.remove(index);
        }
        if self.ids.len() == REMEMBERED {
            self.ids.pop_front();
        }
        self.ids.push_back((address.to_owned(), id.to_owned()));
    }

    fn recall(&self, address: &str) -> Option<String> {
        self.ids
            .iter()
            .rev()
            .find(|(known, _)| known == address)
            .map(|(_, id)| id.clone())
    }
}
