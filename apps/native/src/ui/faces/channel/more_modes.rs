use sdrmm_wire::{
    airtime::{AirtimeSpan, IsmSurveyParams, WifiOccupancyParams},
    ble::BleLink,
    channel::ChannelParams,
    remote_id::RemoteIdLink,
    weather::{LrptMode, WefaxIoc, WefaxLpm},
};
use zgui::prelude::*;

use super::{
    controls::{Ctx, number, segmented, select, titled, toggle},
    lora_view,
    modes::rows,
    tables::{
        AIRTIME_SPANS, BLE_LINKS, REMOTE_ID_LINKS, WEFAX_IOCS, WEFAX_LPMS, lrpt_modes, sonde_types,
    },
};
use crate::ui::kit_channel::NumberSpec;

pub fn mode_view(ctx: Ctx, kind: &str) -> AnyView {
    match kind {
        "apt" => rows(vec![toggle(
            ctx,
            "Partial",
            Some("Keep unfinished passes"),
            ctx.read(get!(Apt, p => p.keep_partial), true),
            set!(Apt, p, v => p.keep_partial = v),
        )]),
        "lrpt" => rows(vec![select(
            ctx,
            "Mode",
            Some("Meteor-M downlink mode"),
            lrpt_modes(),
            ctx.read(get!(Lrpt, p => p.mode), LrptMode::Oqpsk72),
            set!(Lrpt, p, v => p.mode = v),
        )]),
        "wefax" => wefax(ctx),
        "radiosonde" => rows(vec![select(
            ctx,
            "Sonde",
            Some("Sonde type"),
            sonde_types(),
            ctx.read(get!(Radiosonde, p => p.sonde), None),
            set!(Radiosonde, p, v => p.sonde = v),
        )]),
        "ble" => rows(vec![titled(
            ctx,
            "Link",
            Some("Bluetooth channels to listen on"),
            BLE_LINKS,
            ctx.read(get!(Ble, p => p.link), BleLink::Channel),
            set!(Ble, p, v => p.link = v),
        )]),
        "remote_id" => rows(vec![titled(
            ctx,
            "Link",
            Some("Radio link to listen on"),
            REMOTE_ID_LINKS,
            ctx.read(get!(RemoteId, p => p.link), RemoteIdLink::Bluetooth),
            set!(RemoteId, p, v => p.link = v),
        )]),
        "wifi_occupancy" | "ism_survey" => airtime(ctx, kind == "wifi_occupancy"),
        "lora" => lora_view::lora(ctx),
        _ => AnyView::new(()),
    }
}

fn wefax(ctx: Ctx) -> AnyView {
    rows(vec![
        segmented(
            ctx,
            "IOC",
            Some("Index of cooperation"),
            WEFAX_IOCS,
            ctx.read(get!(Wefax, p => p.ioc), WefaxIoc::Ioc576),
            set!(Wefax, p, v => p.ioc = v),
        ),
        segmented(
            ctx,
            "LPM",
            Some("Lines per minute"),
            WEFAX_LPMS,
            ctx.read(get!(Wefax, p => p.lpm), WefaxLpm::Lpm120),
            set!(Wefax, p, v => p.lpm = v),
        ),
        toggle(
            ctx,
            "Partial",
            Some("Keep unfinished charts"),
            ctx.read(get!(Wefax, p => p.keep_partial), true),
            set!(Wefax, p, v => p.keep_partial = v),
        ),
    ])
}

fn airtime_span(params: &ChannelParams) -> Option<AirtimeSpan> {
    match params {
        ChannelParams::WifiOccupancy(p) => Some(p.span),
        ChannelParams::IsmSurvey(p) => Some(p.span),
        _ => None,
    }
}

fn airtime_margin(params: &ChannelParams) -> Option<Option<f64>> {
    match params {
        ChannelParams::WifiOccupancy(p) => Some(Some(f64::from(p.margin_db))),
        ChannelParams::IsmSurvey(p) => Some(Some(f64::from(p.margin_db))),
        _ => None,
    }
}

fn airtime(ctx: Ctx, wifi: bool) -> AnyView {
    let margin = if wifi {
        WifiOccupancyParams::default().margin_db
    } else {
        IsmSurveyParams::default().margin_db
    };
    rows(vec![
        titled(
            ctx,
            "Span",
            Some("Width of the window measured, in MHz"),
            AIRTIME_SPANS,
            ctx.read(airtime_span, AirtimeSpan::Mhz20),
            |params: &mut ChannelParams, span: AirtimeSpan| match params {
                ChannelParams::WifiOccupancy(p) => p.span = span,
                ChannelParams::IsmSurvey(p) => p.span = span,
                _ => {}
            },
        ),
        number(
            ctx,
            "Margin",
            Some("How far over the noise floor counts as busy"),
            NumberSpec::new("Busy margin")
                .limit(ctx.limit("margin_db"))
                .unit("dB"),
            ctx.read(airtime_margin, Some(f64::from(margin))),
            |params: &mut ChannelParams, value: Option<f64>| {
                let Some(value) = value else { return };
                match params {
                    ChannelParams::WifiOccupancy(p) => p.margin_db = value as f32,
                    ChannelParams::IsmSurvey(p) => p.margin_db = value as f32,
                    _ => {}
                }
            },
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_airtime_kinds_share_span_and_margin() {
        let wifi = ChannelParams::WifiOccupancy(WifiOccupancyParams::default());
        let survey = ChannelParams::IsmSurvey(IsmSurveyParams {
            span: AirtimeSpan::Mhz80,
            margin_db: 12.0,
        });
        assert_eq!(airtime_span(&wifi), Some(AirtimeSpan::Mhz20));
        assert_eq!(airtime_margin(&wifi), Some(Some(6.0)));
        assert_eq!(airtime_span(&survey), Some(AirtimeSpan::Mhz80));
        assert_eq!(airtime_margin(&survey), Some(Some(12.0)));
        assert_eq!(airtime_span(&ChannelParams::Am(Default::default())), None);
    }
}
