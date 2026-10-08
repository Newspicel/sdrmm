pub(crate) mod ble;
pub(crate) mod odid;
mod tracker;
pub(crate) mod wifi;

use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_wire::{
    ChannelDescriptor, ChannelParams, ChannelSettings, DecoderFamily, RemoteIdLink, RemoteIdParams,
};

use sdrmm_modem::{
    ble::{self as bluetooth, Lane, band::Band},
    wifi::Receiver as Wifi,
};

use self::tracker::Tracker;
use crate::{
    ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, check_rate, datalink,
};

const PASSBAND: f64 = 0.55;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "remote_id".to_owned(),
    name: "Drone Remote ID".to_owned(),
    summary: "Drone ID and position over Bluetooth and Wi-Fi".to_owned(),
    family: DecoderFamily::Aviation,
    bandwidth_hz: RemoteIdLink::default().bandwidth_hz(),
    input_rate_hz: RemoteIdLink::default().input_rate_hz(),
    has_audio: false,
    decoder_kind: Some("remote_id".to_owned()),
    ..ChannelDescriptor::default()
});

fn params(settings: &ChannelSettings) -> Result<RemoteIdParams, ChannelError> {
    match &settings.params {
        ChannelParams::RemoteId(params) => Ok(*params),
        other => Err(ChannelError::InvalidSettings(format!(
            "remote ID channel got {} params",
            other.type_id()
        ))),
    }
}

pub(crate) fn input_rate(params: &RemoteIdParams) -> f64 {
    params.link.input_rate_hz()
}

pub(crate) fn occupied_band(params: &RemoteIdParams) -> (f64, f64) {
    let half = params.link.bandwidth_hz() / 2.0;
    (-half, half)
}

pub(crate) fn channel_filter(params: &RemoteIdParams) -> ChannelFilter {
    match params.link {
        RemoteIdLink::Bluetooth => {
            datalink::channel_filter(input_rate(params), input_rate(params) * PASSBAND / 2.0)
        }
        RemoteIdLink::BluetoothBand | RemoteIdLink::Wifi => ChannelFilter::Passthrough,
    }
}

enum Front {
    Bluetooth(Box<Lane>),
    BluetoothBand(Box<Band>),
    Wifi(Box<Wifi>),
}

fn invalid(error: impl std::fmt::Display) -> ChannelError {
    ChannelError::InvalidSettings(error.to_string())
}

impl Front {
    fn new(link: RemoteIdLink, frequency_hz: f64) -> Result<Self, ChannelError> {
        Ok(match link {
            RemoteIdLink::Bluetooth => {
                let rf = bluetooth::rf_channel_at(frequency_hz).ok_or_else(|| {
                    ChannelError::InvalidSettings(format!(
                        "{:.3} MHz is not a Bluetooth channel: 2402 to 2480 MHz in 2 MHz steps",
                        frequency_hz / 1e6
                    ))
                })?;
                Self::Bluetooth(Box::new(
                    Lane::new(Some(rf), bluetooth::channel_index(rf)).map_err(invalid)?,
                ))
            }
            RemoteIdLink::BluetoothBand => Self::BluetoothBand(Box::new(
                Band::new(link.input_rate_hz(), frequency_hz).map_err(invalid)?,
            )),
            RemoteIdLink::Wifi => Self::Wifi(Box::new(Wifi::new().map_err(invalid)?)),
        })
    }

    fn reset(&mut self) {
        match self {
            Self::Bluetooth(lane) => lane.reset(),
            Self::BluetoothBand(band) => band.reset(),
            Self::Wifi(wifi) => wifi.reset(),
        }
    }
}

pub struct RemoteIdChannel {
    link: RemoteIdLink,
    frequency_hz: f64,
    front: Front,
    tracker: Tracker,
    rejected: u32,
}

impl ChannelRx for RemoteIdChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        let params = params(&settings)?;
        check_rate(ctx, &DESCRIPTOR, input_rate(&params))?;
        Ok(Self {
            link: params.link,
            frequency_hz: settings.frequency_hz,
            front: Front::new(params.link, settings.frequency_hz)?,
            tracker: Tracker::default(),
            rejected: 0,
        })
    }

    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let params = params(&settings)?;
        if params.link.input_rate_hz() != self.link.input_rate_hz() {
            return Err(ChannelError::InvalidSettings(
                "this Remote ID link changes the input rate; rebuild the channel".to_owned(),
            ));
        }
        if params.link != self.link || settings.frequency_hz != self.frequency_hz {
            self.front = Front::new(params.link, settings.frequency_hz)?;
            self.rejected = 0;
            self.link = params.link;
            self.frequency_hz = settings.frequency_hz;
        }
        Ok(())
    }

    fn retuned(&mut self) {
        self.front.reset();
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        match &mut self.front {
            Front::Bluetooth(lane) => lane.process(
                iq,
                &mut ble::Events {
                    tracker: &mut self.tracker,
                    events: &mut out.events,
                },
            ),
            Front::BluetoothBand(band) => band.process(
                iq,
                &mut ble::Events {
                    tracker: &mut self.tracker,
                    events: &mut out.events,
                },
            ),
            Front::Wifi(receiver) => {
                let rejected = receiver.rejected();
                self.tracker.reject(rejected.saturating_sub(self.rejected));
                self.rejected = rejected;
                receiver.process(
                    iq,
                    &mut wifi::Events {
                        tracker: &mut self.tracker,
                        events: &mut out.events,
                        channel: wifi::channel_number(self.frequency_hz),
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;
