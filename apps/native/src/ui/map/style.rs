use std::cell::Cell;

use serde::{Deserialize, Deserializer, Serialize};
use zgui::{prelude::*, reactive::ArcRwSignal};

use crate::ui::shell::Shell;

pub const BLANK_GROUND: u32 = 0x1a_1c_20;
const DARK_INK: (u32, u32) = (0x15_17_1a, 0xff_ff_ff);
const LIGHT_INK: (u32, u32) = (0xf4_f6_f8, 0x0c_0d_0f);
const LIGHT_GROUND: f64 = 0.4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum Preset {
    #[default]
    Liberty,
    Bright,
    Positron,
    Dark,
    Fiord,
    Custom,
}

impl Preset {
    pub const STYLES: [Self; 5] = [
        Self::Liberty,
        Self::Bright,
        Self::Positron,
        Self::Dark,
        Self::Fiord,
    ];

    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Liberty => "liberty",
            Self::Bright => "bright",
            Self::Positron => "positron",
            Self::Dark => "dark",
            Self::Fiord => "fiord",
            Self::Custom => "custom",
        }
    }

    #[must_use]
    pub const fn palette(self) -> Palette {
        match self {
            Self::Liberty | Self::Custom => LIBERTY,
            Self::Bright => BRIGHT,
            Self::Positron => POSITRON,
            Self::Dark => DARK,
            Self::Fiord => FIORD,
        }
    }
}

impl From<String> for Preset {
    fn from(text: String) -> Self {
        Self::STYLES
            .into_iter()
            .chain([Self::Custom])
            .find(|preset| preset.key() == text)
            .unwrap_or_default()
    }
}

impl From<Preset> for String {
    fn from(preset: Preset) -> Self {
        preset.key().to_owned()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BasemapChoice {
    pub preset: Preset,
    pub custom: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MapView {
    pub basemap: BasemapChoice,
    pub tracks: bool,
}

impl Default for MapView {
    fn default() -> Self {
        Self {
            basemap: BasemapChoice::default(),
            tracks: true,
        }
    }
}

pub fn lenient<'de, D: Deserializer<'de>>(deserializer: D) -> Result<MapView, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

#[must_use]
pub fn with_custom(basemap: &BasemapChoice, custom: String) -> BasemapChoice {
    let custom = custom.trim().to_owned();
    if !custom.is_empty() {
        return BasemapChoice {
            preset: Preset::Custom,
            custom,
        };
    }
    let preset = if basemap.preset == Preset::Custom {
        Preset::default()
    } else {
        basemap.preset
    };
    BasemapChoice { preset, custom }
}

#[must_use]
pub fn is_tile_template(url: &str) -> bool {
    url.contains("{z}") || url.contains("{quadkey}")
}

#[must_use]
pub fn is_web_url(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|parsed| matches!(parsed.scheme(), "http" | "https"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub land: u32,
    pub water: u32,
    pub wood: u32,
    pub park: u32,
    pub landuse: u32,
    pub building: u32,
    pub minor: u32,
    pub secondary: u32,
    pub primary: u32,
    pub motorway: u32,
    pub rail: u32,
    pub region: u32,
    pub border: u32,
    pub place: u32,
    pub place_faint: u32,
}

const LIBERTY: Palette = Palette {
    land: 0xf8_f4_f0,
    water: 0x9e_bd_ff,
    wood: 0xd8_e8_c8,
    park: 0xd8_e8_c8,
    landuse: 0xee_ea_e4,
    building: 0xdf_db_d7,
    minor: 0xff_ff_ff,
    secondary: 0xff_ee_aa,
    primary: 0xff_ee_aa,
    motorway: 0xff_cc_88,
    rail: 0xbb_bb_bb,
    region: 0x9e_9c_ab,
    border: 0x9e_9c_ab,
    place: 0x33_33_33,
    place_faint: 0x66_66_66,
};

const BRIGHT: Palette = Palette {
    land: 0xf8_f4_f0,
    water: 0xae_cf_e2,
    wood: 0xd6_e5_c3,
    park: 0xd8_e8_c8,
    landuse: 0xef_eb_e5,
    building: 0xe6_df_d6,
    minor: 0xff_ff_ff,
    secondary: 0xff_ee_aa,
    primary: 0xff_ee_aa,
    motorway: 0xff_cc_88,
    rail: 0xbb_bb_bb,
    region: 0x9e_9c_ab,
    border: 0x9e_9c_ab,
    place: 0x33_33_33,
    place_faint: 0x66_66_66,
};

const POSITRON: Palette = Palette {
    land: 0xf2_f3_f0,
    water: 0xc2_c8_ca,
    wood: 0xe4_e9_de,
    park: 0xe6_eb_e0,
    landuse: 0xea_ea_e6,
    building: 0xe4_e4_e4,
    minor: 0xff_ff_ff,
    secondary: 0xff_ff_ff,
    primary: 0xff_ff_ff,
    motorway: 0xff_ff_ff,
    rail: 0xdd_dd_dd,
    region: 0xb8_b8_c0,
    border: 0x9e_9c_ab,
    place: 0x55_55_55,
    place_faint: 0x88_88_88,
};

const DARK: Palette = Palette {
    land: 0x0c_0c_0c,
    water: 0x1b_1b_1d,
    wood: 0x12_16_12,
    park: 0x11_16_11,
    landuse: 0x14_14_14,
    building: 0x1d_1d_1d,
    minor: 0x1e_1e_1e,
    secondary: 0x26_26_26,
    primary: 0x2f_2f_2f,
    motorway: 0x3a_3a_3a,
    rail: 0x2a_2a_2a,
    region: 0x44_44_44,
    border: 0x55_55_55,
    place: 0xb0_b0_b0,
    place_faint: 0x70_70_70,
};

const FIORD: Palette = Palette {
    land: 0x45_51_6e,
    water: 0x38_43_5c,
    wood: 0x4a_57_70,
    park: 0x4c_58_72,
    landuse: 0x49_55_71,
    building: 0x003e_4a64,
    minor: 0x56_63_7e,
    secondary: 0x5d_6a_86,
    primary: 0x67_74_8f,
    motorway: 0x7a_87_9f,
    rail: 0x5a_66_80,
    region: 0x80_90_a8,
    border: 0x9a_a8_bf,
    place: 0xd0_d6_e0,
    place_faint: 0xa0_aa_bc,
};

#[must_use]
pub fn label_ink(ground: u32) -> (u32, u32) {
    let linear = |shift: u32| {
        let channel = f64::from((ground >> shift) & 0xff) / 255.0;
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    };
    let luminance = 0.2126 * linear(16) + 0.7152 * linear(8) + 0.0722 * linear(0);
    if luminance > LIGHT_GROUND {
        DARK_INK
    } else {
        LIGHT_INK
    }
}

#[must_use]
pub fn hex(colour: u32) -> String {
    format!("#{colour:06x}")
}

#[must_use]
pub fn halo(colour: u32) -> String {
    let shade = hex(colour);
    format!("0 0 2px {shade}, 0 0 2px {shade}, 0 0 1px {shade}")
}

struct Held {
    view: ArcRwSignal<MapView>,
    loaded: Cell<bool>,
}

thread_local! {
    static HELD: Held = Held {
        view: ArcRwSignal::new(MapView::default()),
        loaded: Cell::new(false),
    };
}

#[must_use]
pub fn map_view() -> ArcRwSignal<MapView> {
    HELD.with(|held| {
        if !held.loaded.replace(true)
            && let Some(shell) = Shell::current()
        {
            let mut stored = None;
            shell.remember(|prefs| stored = Some(prefs.map_view.clone()));
            if let Some(stored) = stored {
                held.view.set(stored);
            }
        }
        held.view.clone()
    })
}

pub fn set_map_view(next: MapView) {
    let view = map_view();
    if let Some(shell) = Shell::current() {
        let saved = next.clone();
        shell.remember(move |prefs| prefs.map_view = saved);
    }
    view.set(next);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_preset_reads_as_the_default_and_a_custom_url_wins() {
        let view: MapView =
            serde_json::from_str(r#"{"basemap":{"preset":"neon","custom":""},"tracks":false}"#)
                .expect("a view");
        assert_eq!(view.basemap.preset, Preset::Liberty);
        assert!(!view.tracks);
        let back: MapView = serde_json::from_value(
            serde_json::to_value(MapView {
                basemap: BasemapChoice {
                    preset: Preset::Fiord,
                    custom: String::new(),
                },
                tracks: true,
            })
            .expect("json"),
        )
        .expect("a view");
        assert_eq!(back.basemap.preset, Preset::Fiord);
        assert_eq!(MapView::default().basemap.preset, Preset::Liberty);
    }

    #[test]
    fn a_broken_view_falls_back_without_losing_the_rest() {
        #[derive(Deserialize)]
        struct Holder {
            #[serde(default, deserialize_with = "lenient")]
            view: MapView,
        }
        let held: Holder = serde_json::from_str(r#"{"view":{"tracks":"yes"}}"#).expect("held");
        assert_eq!(held.view, MapView::default());
    }

    #[test]
    fn clearing_the_custom_url_returns_to_a_preset() {
        let custom = with_custom(
            &BasemapChoice::default(),
            " https://t/{z}/{x}/{y}.png ".into(),
        );
        assert_eq!(custom.preset, Preset::Custom);
        assert_eq!(custom.custom, "https://t/{z}/{x}/{y}.png");
        assert_eq!(with_custom(&custom, String::new()).preset, Preset::Liberty);
        let dark = BasemapChoice {
            preset: Preset::Dark,
            custom: String::new(),
        };
        assert_eq!(with_custom(&dark, String::new()).preset, Preset::Dark);
    }

    #[test]
    fn templates_and_web_urls_are_told_apart() {
        assert!(is_tile_template("https://a/{z}/{x}/{y}.png"));
        assert!(is_tile_template("https://a/{quadkey}.jpeg"));
        assert!(!is_tile_template("https://a/style.json"));
        assert!(is_web_url("https://a/style.json"));
        assert!(!is_web_url("file:///etc/passwd"));
        assert!(!is_web_url("not a url"));
    }

    #[test]
    fn labels_are_dark_on_light_ground_and_light_on_dark() {
        assert_eq!(label_ink(Preset::Liberty.palette().land), DARK_INK);
        assert_eq!(label_ink(Preset::Dark.palette().land), LIGHT_INK);
        assert_eq!(label_ink(Preset::Fiord.palette().land), LIGHT_INK);
        assert_eq!(label_ink(BLANK_GROUND), LIGHT_INK);
    }
}
