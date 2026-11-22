use sdrmm_decoder::registry::Native;
use sdrmm_wire::FilterSpec;
use sdrmm_wire::{Control, KindSpec, SettingField};
mod model;
#[cfg(any(test, feature = "synth"))]
pub mod synth;
pub use model::*;
pub(crate) mod ble;
pub(crate) mod odid;
mod tracker;
pub(crate) mod wifi;

use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_modem::wifi::{Receiver as Wifi, channel::channel_at};
use sdrmm_wire::{ChannelDescriptor, ChannelSettings, DecoderFamily, EventKind};

use self::tracker::Tracker;
use sdrmm_decoder::{Channel, ChannelCtx, ChannelError, ChannelOutputs, NativeChannel, check_rate};
use sdrmm_modem::ble as bluetooth;
use sdrmm_modem::cpm::Mapping;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "remote_id".to_owned(),
    name: "Drone Remote ID".to_owned(),
    summary: "Drone ID and position over Bluetooth and Wi-Fi".to_owned(),
    family: DecoderFamily::AVIATION,
    verification: sdrmm_wire::Verification::ReferenceFixture,
    bandwidth_hz: RemoteIdLink::default().bandwidth_hz(),
    input_rate_hz: RemoteIdLink::default().input_rate_hz(),
    has_audio: false,
    emits: vec![EventKind::named("remote_id")],
    ..ChannelDescriptor::default()
});

fn params(settings: &ChannelSettings) -> Result<RemoteIdParams, ChannelError> {
    sdrmm_decoder::settings_of::<RemoteIdParams>(settings)
}

pub(crate) fn input_rate(params: RemoteIdParams) -> f64 {
    params.link.input_rate_hz()
}

const CHANNEL_TAPS: usize = 129;

pub(crate) fn occupied_band(params: RemoteIdParams) -> (f64, f64) {
    let half = params.link.bandwidth_hz() / 2.0;
    (-half, half)
}

pub(crate) fn channel_filter(params: RemoteIdParams) -> FilterSpec {
    match params.link {
        RemoteIdLink::Bluetooth => {
            FilterSpec::lowpass(CHANNEL_TAPS, bluetooth::CHANNEL_HALF_PASSBAND_HZ)
        }
        RemoteIdLink::BluetoothBand | RemoteIdLink::Wifi => FilterSpec::Passthrough,
    }
}

enum Front {
    Bluetooth(bluetooth::Front),
    Wifi(Box<Wifi>),
}

fn invalid(error: impl std::fmt::Display) -> ChannelError {
    ChannelError::InvalidSettings(error.to_string())
}

impl Front {
    fn new(link: RemoteIdLink, frequency_hz: f64) -> Result<Self, ChannelError> {
        Ok(match link {
            RemoteIdLink::Bluetooth => {
                Self::Bluetooth(bluetooth::Front::channel(frequency_hz).map_err(invalid)?)
            }
            RemoteIdLink::BluetoothBand => {
                Self::Bluetooth(bluetooth::Front::band(frequency_hz).map_err(invalid)?)
            }
            RemoteIdLink::Wifi => Self::Wifi(Box::new(Wifi::new().map_err(invalid)?)),
        })
    }

    fn reset(&mut self) {
        match self {
            Self::Bluetooth(front) => front.reset(),
            Self::Wifi(wifi) => wifi.reset(),
        }
    }

    fn rejected(&self) -> u32 {
        match self {
            Self::Bluetooth(front) => front.rejected(),
            Self::Wifi(wifi) => wifi.rejected(),
        }
    }
}

pub struct RemoteIdChannel {
    link: RemoteIdLink,
    frequency_hz: f64,
    front: Front,
    tracker: Tracker,
    mapping: Mapping,
}

impl NativeChannel for RemoteIdChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }
    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        let params = params(&settings)?;
        check_rate(ctx, &DESCRIPTOR, input_rate(params))?;
        Ok(Self {
            link: params.link,
            frequency_hz: settings.frequency_hz,
            front: Front::new(params.link, settings.frequency_hz)?,
            tracker: Tracker::default(),
            mapping: Mapping::natural(2),
        })
    }
}

impl Channel for RemoteIdChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let params = params(&settings)?;
        if params.link.input_rate_hz() != self.link.input_rate_hz() {
            return Err(ChannelError::InvalidSettings(
                "this Remote ID link changes the input rate; rebuild the channel".to_owned(),
            ));
        }
        if params.link != self.link || settings.frequency_hz != self.frequency_hz {
            self.front = Front::new(params.link, settings.frequency_hz)?;
            self.tracker.clear_rejected();
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
            Front::Bluetooth(front) => front.process(
                iq,
                &mut ble::Events {
                    tracker: &mut self.tracker,
                    out: &mut out.events,
                    tap: &mut out.symbols,
                    mapping: &self.mapping,
                },
            ),
            Front::Wifi(receiver) => receiver.process(
                iq,
                &mut wifi::Events {
                    tracker: &mut self.tracker,
                    out: &mut out.events,
                    channel: channel_at(self.frequency_hz).map(|channel| channel.number),
                },
            ),
        }
    }

    fn rejected(&self) -> u32 {
        self.front
            .rejected()
            .saturating_add(self.tracker.rejected())
    }
}

pub(crate) fn remote_id_decoder() -> Native<RemoteIdParams> {
    let decoder = Native::new::<RemoteIdChannel>(|p| occupied_band(*p))
        .filter(|p| Ok(channel_filter(*p)))
        .input_rate(|p| input_rate(*p))
        .home(2_426_000_000.0)
        .form(remote_id_form)
        .kinds(remote_id_kinds);
    cfg_select! {
        any(test, feature = "synth") => decoder.synth(synth::SIGNALS),
        _ => decoder,
    }
}

pub(crate) fn remote_id_form() -> Vec<SettingField> {
    vec![
        SettingField::new("link", "Link", Control::choice::<RemoteIdLink>())
            .title("Radio link to listen on"),
    ]
}

fn remote_id_kinds() -> Vec<KindSpec> {
    vec![crate::remote_id::remote_id_kind()]
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;
