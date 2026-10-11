use sdrmm_wire::SarsatProtocol;

use super::Bits;

#[derive(Clone, Copy)]
struct Span {
    first: u32,
    last: u32,
}

const fn span(first: u32, last: u32) -> Span {
    Span { first, last }
}

impl Span {
    const fn get(self, bits: Bits) -> u32 {
        bits.get(self.first, self.last)
    }

    const fn ones(self) -> u32 {
        Bits::mask(self.first, self.last) as u32
    }
}

#[derive(Clone, Copy)]
struct Coarse {
    flag: u32,
    degrees: Span,
    per_degree: u32,
    minutes: Option<(Span, u32)>,
}

#[derive(Clone, Copy)]
struct Offset {
    sign: u32,
    minutes: Span,
    seconds: Span,
}

#[derive(Clone, Copy)]
struct Axis {
    coarse: Coarse,
    offset: Option<Offset>,
    limit: f64,
}

#[derive(Clone, Copy)]
enum Offsets {
    Never,
    Always,
    Flagged(u32),
    UnlessZero(Span),
}

#[derive(Clone, Copy)]
struct Layout {
    lat: Axis,
    lon: Axis,
    offsets: Offsets,
    in_pdf2: bool,
}

const UNKNOWN_SECONDS: u32 = 15;

const STANDARD: Layout = Layout {
    lat: Axis {
        coarse: Coarse {
            flag: 65,
            degrees: span(66, 74),
            per_degree: 4,
            minutes: None,
        },
        offset: Some(Offset {
            sign: 113,
            minutes: span(114, 118),
            seconds: span(119, 122),
        }),
        limit: 90.0,
    },
    lon: Axis {
        coarse: Coarse {
            flag: 75,
            degrees: span(76, 85),
            per_degree: 4,
            minutes: None,
        },
        offset: Some(Offset {
            sign: 123,
            minutes: span(124, 128),
            seconds: span(129, 132),
        }),
        limit: 180.0,
    },
    offsets: Offsets::Always,
    in_pdf2: false,
};

const NATIONAL: Layout = Layout {
    lat: Axis {
        coarse: Coarse {
            flag: 59,
            degrees: span(60, 66),
            per_degree: 1,
            minutes: Some((span(67, 71), 2)),
        },
        offset: Some(Offset {
            sign: 113,
            minutes: span(114, 115),
            seconds: span(116, 119),
        }),
        limit: 90.0,
    },
    lon: Axis {
        coarse: Coarse {
            flag: 72,
            degrees: span(73, 80),
            per_degree: 1,
            minutes: Some((span(81, 85), 2)),
        },
        offset: Some(Offset {
            sign: 120,
            minutes: span(121, 122),
            seconds: span(123, 126),
        }),
        limit: 180.0,
    },
    offsets: Offsets::Flagged(110),
    in_pdf2: false,
};

const RLS: Layout = Layout {
    lat: Axis {
        coarse: Coarse {
            flag: 67,
            degrees: span(68, 75),
            per_degree: 2,
            minutes: None,
        },
        offset: Some(Offset {
            sign: 115,
            minutes: span(116, 119),
            seconds: span(120, 123),
        }),
        limit: 90.0,
    },
    lon: Axis {
        coarse: Coarse {
            flag: 76,
            degrees: span(77, 85),
            per_degree: 2,
            minutes: None,
        },
        offset: Some(Offset {
            sign: 124,
            minutes: span(125, 128),
            seconds: span(129, 132),
        }),
        limit: 180.0,
    },
    offsets: Offsets::Always,
    in_pdf2: false,
};

const ELT_DT: Layout = Layout {
    offsets: Offsets::UnlessZero(span(113, 114)),
    ..RLS
};

const USER: Layout = Layout {
    lat: Axis {
        coarse: Coarse {
            flag: 108,
            degrees: span(109, 115),
            per_degree: 1,
            minutes: Some((span(116, 119), 4)),
        },
        offset: None,
        limit: 90.0,
    },
    lon: Axis {
        coarse: Coarse {
            flag: 120,
            degrees: span(121, 128),
            per_degree: 1,
            minutes: Some((span(129, 132), 4)),
        },
        offset: None,
        limit: 180.0,
    },
    offsets: Offsets::Never,
    in_pdf2: true,
};

const fn layout(protocol: SarsatProtocol) -> Option<Layout> {
    match protocol {
        SarsatProtocol::StandardLocation
        | SarsatProtocol::ShipSecurity
        | SarsatProtocol::StandardTest => Some(STANDARD),
        SarsatProtocol::NationalLocation | SarsatProtocol::NationalTest => Some(NATIONAL),
        SarsatProtocol::RlsLocation => Some(RLS),
        SarsatProtocol::EltDtLocation => Some(ELT_DT),
        SarsatProtocol::MaritimeUser
        | SarsatProtocol::RadioCallSignUser
        | SarsatProtocol::AviationUser
        | SarsatProtocol::SerialUser
        | SarsatProtocol::TestUser => Some(USER),
        SarsatProtocol::NationalUser
        | SarsatProtocol::Orbitography
        | SarsatProtocol::SecondGeneration
        | SarsatProtocol::Spare => None,
    }
}

pub(super) fn position(bits: Bits, protocol: SarsatProtocol, second: bool) -> Option<(f64, f64)> {
    let layout = layout(protocol)?;
    if layout.in_pdf2 && !second {
        return None;
    }
    let fine = second
        && match layout.offsets {
            Offsets::Never => false,
            Offsets::Always => true,
            Offsets::Flagged(bit) => bits.bit(bit),
            Offsets::UnlessZero(field) => field.get(bits) != 0,
        };
    Some((axis(bits, layout.lat, fine)?, axis(bits, layout.lon, fine)?))
}

fn axis(bits: Bits, axis: Axis, fine: bool) -> Option<f64> {
    let coarse = axis.coarse;
    let raw = coarse.degrees.get(bits);
    if raw == coarse.degrees.ones() {
        return None;
    }
    let minutes = match coarse.minutes {
        Some((field, step)) => field.get(bits) * step,
        None => 0,
    };
    if minutes >= 60 {
        return None;
    }
    let mut degrees = f64::from(raw) / f64::from(coarse.per_degree) + f64::from(minutes) / 60.0;
    if let Some(offset) = axis.offset.filter(|_| fine) {
        degrees += offset_degrees(bits, offset);
    }
    if !(0.0..=axis.limit).contains(&degrees) {
        return None;
    }
    Some(if bits.bit(coarse.flag) {
        -degrees
    } else {
        degrees
    })
}

fn offset_degrees(bits: Bits, offset: Offset) -> f64 {
    let seconds = match offset.seconds.get(bits) {
        UNKNOWN_SECONDS => 0,
        quarters => quarters * 4,
    };
    let magnitude = f64::from(offset.minutes.get(bits)) / 60.0 + f64::from(seconds) / 3_600.0;
    if bits.bit(offset.sign) {
        magnitude
    } else {
        -magnitude
    }
}

pub(super) fn defaults(protocol: SarsatProtocol) -> Vec<(u32, u32, u128)> {
    let Some(layout) = layout(protocol).filter(|layout| !layout.in_pdf2) else {
        return Vec::new();
    };
    [layout.lat.coarse, layout.lon.coarse]
        .into_iter()
        .flat_map(|coarse| {
            let degrees = coarse.degrees;
            [
                Some((coarse.flag, coarse.flag, 0)),
                Some((degrees.first, degrees.last, u128::from(degrees.ones()))),
                coarse
                    .minutes
                    .map(|(minutes, _)| (minutes.first, minutes.last, 0)),
            ]
        })
        .flatten()
        .collect()
}

#[cfg(any(test, feature = "synth"))]
pub(crate) fn encode(bits: Bits, protocol: SarsatProtocol, lat: f64, lon: f64) -> Bits {
    let Some(layout) = layout(protocol) else {
        return bits;
    };
    let bits = match layout.offsets {
        Offsets::Flagged(bit) => bits.with(bit, bit, 1),
        Offsets::UnlessZero(field) => bits.with(field.first, field.last, 0b11),
        Offsets::Never | Offsets::Always => bits,
    };
    let bits = encode_axis(bits, layout.lat, lat);
    encode_axis(bits, layout.lon, lon)
}

#[cfg(any(test, feature = "synth"))]
fn encode_axis(bits: Bits, axis: Axis, value: f64) -> Bits {
    let coarse = axis.coarse;
    let step_minutes = match coarse.minutes {
        Some((_, step)) => step,
        None => 60 / coarse.per_degree,
    };
    let total_minutes =
        (value.abs() * 60.0 / f64::from(step_minutes)).round() as u32 * step_minutes;
    let raw_degrees = match coarse.minutes {
        Some(_) => total_minutes / 60,
        None => total_minutes / step_minutes,
    };
    let mut bits = bits
        .with(coarse.flag, coarse.flag, u128::from(value < 0.0))
        .with(
            coarse.degrees.first,
            coarse.degrees.last,
            u128::from(raw_degrees),
        );
    if let Some((field, step)) = coarse.minutes {
        bits = bits.with(
            field.first,
            field.last,
            u128::from(total_minutes % 60 / step),
        );
    }
    match axis.offset {
        Some(offset) => encode_offset(bits, offset, value, total_minutes),
        None => bits,
    }
}

#[cfg(any(test, feature = "synth"))]
fn encode_offset(bits: Bits, offset: Offset, value: f64, coarse_minutes: u32) -> Bits {
    let rounded = (value.abs() * 900.0).round() as i64 * 4;
    let delta = rounded - i64::from(coarse_minutes) * 60;
    let magnitude = delta.unsigned_abs();
    let (sign, minutes, quarters) = if magnitude == 0 {
        (1, 0, u64::from(UNKNOWN_SECONDS))
    } else {
        (u64::from(delta > 0), magnitude / 60, magnitude % 60 / 4)
    };
    bits.with(offset.sign, offset.sign, u128::from(sign))
        .with(
            offset.minutes.first,
            offset.minutes.last,
            u128::from(minutes),
        )
        .with(
            offset.seconds.first,
            offset.seconds.last,
            u128::from(quarters),
        )
}
