use sdrmm_wire::weather::{AprsWeather, AptImage, LrptImage, RadiosondeFrame, WefaxPicture};

use super::{EventDetail, detail, repaired, some};
use crate::decoders::{
    text::{
        celsius, climb_rate, degrees, hectopascal, metres, metres_per_second, millimetres,
        position, seconds, whole_percent,
    },
    views::wind,
};

fn state(complete: bool) -> Option<String> {
    some(if complete { "complete" } else { "cut short" })
}

#[must_use]
pub fn avhrr_channels(image: &AptImage) -> Option<String> {
    if image.channel_a.is_none() && image.channel_b.is_none() {
        return None;
    }
    let name =
        |channel: Option<sdrmm_wire::weather::AvhrrChannel>| channel.map_or("?", |c| c.label());
    Some(format!(
        "ch {} + {}",
        name(image.channel_a),
        name(image.channel_b)
    ))
}

pub fn apt(p: &AptImage) -> EventDetail {
    detail(
        [
            ("Channels", avhrr_channels(p)),
            ("Lines received", some(p.lines.to_string())),
            ("State", state(p.complete)),
            ("Took", some(seconds(p.duration_ms))),
        ],
        None,
    )
}

pub fn lrpt(p: &LrptImage) -> EventDetail {
    let apids: Vec<String> = p.apids.iter().map(u16::to_string).collect();
    detail(
        [
            ("Mode", some(p.mode.label())),
            ("Size", some(format!("{} × {}", p.width, p.lines))),
            ("APIDs", some(apids.join(", "))),
            ("Frames", some(p.frames.to_string())),
            ("Frames repaired", some(p.frames_corrected.to_string())),
            ("Frames failed", some(p.frames_failed.to_string())),
            ("Packets lost", some(p.packets_lost.to_string())),
            ("State", state(p.complete)),
            ("Took", some(seconds(p.duration_ms))),
        ],
        None,
    )
}

pub fn wefax(p: &WefaxPicture) -> EventDetail {
    detail(
        [
            ("IOC", some(p.ioc.value().to_string())),
            ("Speed", some(format!("{} LPM", p.lpm.value()))),
            ("Size", some(format!("{} × {}", p.width, p.lines))),
            ("State", state(p.complete)),
            ("Took", some(seconds(p.duration_ms))),
        ],
        None,
    )
}

pub fn radiosonde(f: &RadiosondeFrame) -> EventDetail {
    detail(
        [
            ("Serial", some(&f.serial)),
            ("Type", some(f.sonde.label())),
            ("Frame", f.frame.map(|n| n.to_string())),
            ("Time", f.time.clone()),
            ("Position", position(f.lat, f.lon)),
            ("Altitude", metres(f.altitude_m)),
            ("Climb", climb_rate(f.climb_ms)),
            ("Speed", metres_per_second(f.speed_ms)),
            ("Heading", degrees(f.heading_deg)),
            ("Temperature", celsius(f.temperature_c)),
            ("Humidity", whole_percent(f.humidity_pct.map(f64::from))),
            ("Pressure", hectopascal(f.pressure_hpa)),
            ("Satellites", f.satellites.map(|n| n.to_string())),
            ("Battery", f.battery_v.map(|v| format!("{v:.2} V"))),
            ("Repaired", repaired(f.errors_corrected, "")),
            ("Frames lost", repaired(f.rejected, "")),
        ],
        None,
    )
}

#[must_use]
pub fn aprs_rows(weather: &AprsWeather) -> Vec<(&'static str, Option<String>)> {
    vec![
        ("Wind", wind(weather)),
        (
            "Gust",
            metres_per_second(weather.wind_gust_ms.map(f64::from)),
        ),
        ("Temperature", celsius(weather.temperature_c)),
        (
            "Humidity",
            whole_percent(weather.humidity_pct.map(f64::from)),
        ),
        ("Pressure", hectopascal(weather.pressure_hpa)),
        ("Rain 1 h", millimetres(weather.rain_1h_mm)),
        ("Rain 24 h", millimetres(weather.rain_24h_mm)),
        ("Rain since midnight", millimetres(weather.rain_midnight_mm)),
        ("Snow 24 h", millimetres(weather.snow_24h_mm)),
        (
            "Luminosity",
            weather.luminosity_wm2.map(|w| format!("{w} W/m²")),
        ),
    ]
}
