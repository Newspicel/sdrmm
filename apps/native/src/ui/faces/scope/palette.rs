use super::colormap::{COLORMAPS, Colormap, Rgb};

pub const CUSTOM_STOPS: usize = 5;
pub const CUSTOM_KEY: &str = "custom";
pub const CUSTOM_INDEX: u32 = COLORMAPS.len() as u32;

pub const DEFAULT_CUSTOM: [Rgb; CUSTOM_STOPS] = [
    [0.0, 0.0, 0.12549],
    [0.0, 0.0, 0.56863],
    [0.11765, 0.56471, 1.0],
    [1.0, 1.0, 0.0],
    [1.0, 0.0, 0.0],
];

const GRADIENT_STOPS: u32 = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Palette {
    Map(Colormap),
    Custom([Rgb; CUSTOM_STOPS]),
}

impl Default for Palette {
    fn default() -> Self {
        Self::Map(Colormap::default())
    }
}

impl From<Colormap> for Palette {
    fn from(map: Colormap) -> Self {
        Self::Map(map)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteChoice {
    Map(Colormap),
    Custom,
}

impl Default for PaletteChoice {
    fn default() -> Self {
        Self::Map(Colormap::default())
    }
}

impl PaletteChoice {
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Map(map) => map.name(),
            Self::Custom => CUSTOM_KEY,
        }
    }

    #[must_use]
    pub fn parse(key: &str) -> Self {
        if key == CUSTOM_KEY {
            return Self::Custom;
        }
        COLORMAPS
            .iter()
            .find(|map| map.name() == key)
            .map_or_else(Self::default, |map| Self::Map(*map))
    }

    #[must_use]
    pub fn palette(self, custom: [Rgb; CUSTOM_STOPS]) -> Palette {
        match self {
            Self::Map(map) => Palette::Map(map),
            Self::Custom => Palette::Custom(custom),
        }
    }
}

impl Palette {
    #[must_use]
    pub fn sample(self, t: f64) -> Rgb {
        match self {
            Self::Map(map) => map.sample(t),
            Self::Custom(stops) => sample_stops(&stops, unit(t)),
        }
    }

    #[must_use]
    pub fn shader_index(self) -> u32 {
        match self {
            Self::Map(map) => map.index(),
            Self::Custom(_) => CUSTOM_INDEX,
        }
    }

    #[must_use]
    pub fn custom_stops(self) -> [Rgb; CUSTOM_STOPS] {
        match self {
            Self::Map(_) => DEFAULT_CUSTOM,
            Self::Custom(stops) => stops,
        }
    }

    #[must_use]
    pub fn lut(self) -> Vec<u8> {
        (0..256)
            .flat_map(|step| {
                self.sample(f64::from(step) / 255.0)
                    .map(|channel| (channel * 255.0).round() as u8)
            })
            .collect()
    }

    #[must_use]
    pub fn gradient(self, direction: &str) -> String {
        let stops: Vec<String> = (0..GRADIENT_STOPS)
            .map(|at| {
                let [r, g, b] = self.sample(f64::from(at) / f64::from(GRADIENT_STOPS - 1));
                format!(
                    "rgb({} {} {})",
                    (r * 255.0).round(),
                    (g * 255.0).round(),
                    (b * 255.0).round()
                )
            })
            .collect();
        format!("linear-gradient({direction}, {})", stops.join(", "))
    }
}

fn unit(t: f64) -> f64 {
    if t.is_finite() {
        t.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[must_use]
pub fn sample_stops(stops: &[Rgb], t: f64) -> Rgb {
    match stops {
        [] => [0.0; 3],
        [only] => *only,
        _ => {
            let x = t * (stops.len() - 1) as f64;
            let at = (x.floor().max(0.0) as usize).min(stops.len() - 2);
            let (low, high) = (stops[at], stops[at + 1]);
            let f = x - at as f64;
            [
                low[0] + (high[0] - low[0]) * f,
                low[1] + (high[1] - low[1]) * f,
                low[2] + (high[2] - low[2]) * f,
            ]
        }
    }
}

#[must_use]
pub fn hex_to_rgb(hex: &str) -> Option<Rgb> {
    let digits = hex.strip_prefix('#').unwrap_or(hex);
    if digits.len() != 6 || !digits.chars().all(|digit| digit.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |at: usize| {
        u8::from_str_radix(&digits[at..at + 2], 16)
            .ok()
            .map(|byte| f64::from(byte) / 255.0)
    };
    Some([channel(0)?, channel(2)?, channel(4)?])
}

#[must_use]
pub fn rgb_to_hex(rgb: Rgb) -> String {
    let [r, g, b] = rgb.map(|channel| (unit(channel) * 255.0).round() as u8);
    format!("#{r:02x}{g:02x}{b:02x}")
}

#[must_use]
pub fn parse_custom(hexes: &[String]) -> Option<[Rgb; CUSTOM_STOPS]> {
    if hexes.len() != CUSTOM_STOPS {
        return None;
    }
    let mut stops = DEFAULT_CUSTOM;
    for (slot, hex) in stops.iter_mut().zip(hexes) {
        *slot = hex_to_rgb(hex)?;
    }
    Some(stops)
}

#[must_use]
pub fn custom_hexes(stops: &[Rgb; CUSTOM_STOPS]) -> Vec<String> {
    stops.iter().map(|stop| rgb_to_hex(*stop)).collect()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hsv {
    pub h: f64,
    pub s: f64,
    pub v: f64,
}

#[must_use]
pub fn rgb_to_hsv([r, g, b]: Rgb) -> Hsv {
    let max = r.max(g).max(b);
    let delta = max - r.min(g).min(b);
    Hsv {
        h: hue_of([r, g, b], max, delta),
        s: if max == 0.0 { 0.0 } else { delta / max },
        v: max,
    }
}

#[must_use]
pub fn hsv_to_rgb(Hsv { h, s, v }: Hsv) -> Rgb {
    let channel = |n: f64| {
        let k = (n + h / 60.0) % 6.0;
        v - v * s * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    [channel(5.0), channel(3.0), channel(1.0)]
}

fn hue_of([r, g, b]: Rgb, max: f64, delta: f64) -> f64 {
    if delta == 0.0 {
        return 0.0;
    }
    let sector = if max == r {
        (g - b) / delta
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };
    (sector * 60.0 + 360.0) % 360.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: Rgb, b: Rgb) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    #[test]
    fn a_custom_palette_runs_through_its_stops() {
        let custom = Palette::Custom(DEFAULT_CUSTOM);
        assert!(near(custom.sample(0.0), DEFAULT_CUSTOM[0]));
        assert!(near(custom.sample(0.5), DEFAULT_CUSTOM[2]));
        assert!(near(custom.sample(1.0), DEFAULT_CUSTOM[4]));
        assert!(near(custom.sample(f64::NAN), DEFAULT_CUSTOM[0]));
        let between = custom.sample(0.125);
        assert!((between[2] - (0.12549 + 0.56863) / 2.0).abs() < 1e-9);
    }

    #[test]
    fn the_shader_index_puts_custom_after_every_named_map() {
        assert_eq!(Palette::Map(Colormap::Gray).shader_index(), 5);
        assert_eq!(Palette::Custom(DEFAULT_CUSTOM).shader_index(), 6);
        assert_eq!(Palette::Map(Colormap::Magma).custom_stops(), DEFAULT_CUSTOM);
    }

    #[test]
    fn a_choice_reads_back_from_its_key() {
        for map in COLORMAPS {
            assert_eq!(PaletteChoice::parse(map.name()).key(), map.name());
        }
        assert_eq!(PaletteChoice::parse("custom"), PaletteChoice::Custom);
        assert_eq!(PaletteChoice::parse("rainbow").key(), "classic");
        assert_eq!(
            PaletteChoice::default(),
            PaletteChoice::Map(Colormap::Classic)
        );
        assert_eq!(
            PaletteChoice::Custom.palette(DEFAULT_CUSTOM),
            Palette::Custom(DEFAULT_CUSTOM)
        );
    }

    #[test]
    fn hex_round_trips_and_rejects_junk() {
        assert_eq!(rgb_to_hex([1.0, 0.0, 0.5]), "#ff0080");
        assert_eq!(
            hex_to_rgb("#ff0080").map(rgb_to_hex).as_deref(),
            Some("#ff0080")
        );
        assert_eq!(hex_to_rgb("00ff00"), Some([0.0, 1.0, 0.0]));
        assert_eq!(hex_to_rgb("#fff"), None);
        assert_eq!(hex_to_rgb("#gggggg"), None);
        assert_eq!(rgb_to_hex([2.0, -1.0, f64::NAN]), "#ff0000");
    }

    #[test]
    fn stored_stops_need_all_five_readable() {
        let hexes = custom_hexes(&DEFAULT_CUSTOM);
        assert_eq!(hexes.len(), CUSTOM_STOPS);
        assert!(parse_custom(&hexes).is_some());
        assert_eq!(parse_custom(&hexes[..4]), None);
        let mut broken = hexes;
        broken[2] = String::from("nope");
        assert_eq!(parse_custom(&broken), None);
    }

    #[test]
    fn hsv_round_trips_the_primaries_and_greys() {
        for rgb in [
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.5, 0.5, 0.5],
            [0.2, 0.4, 0.6],
        ] {
            assert!(near(hsv_to_rgb(rgb_to_hsv(rgb)), rgb), "{rgb:?}");
        }
        assert_eq!(rgb_to_hsv([0.0, 0.0, 1.0]).h, 240.0);
        assert_eq!(rgb_to_hsv([0.0, 0.0, 0.0]).s, 0.0);
    }

    #[test]
    fn a_gradient_names_eight_stops() {
        let gradient = Palette::Map(Colormap::Gray).gradient("to right");
        assert!(gradient.starts_with("linear-gradient(to right, rgb(0 0 0)"));
        assert_eq!(gradient.matches("rgb(").count(), 8);
    }
}
