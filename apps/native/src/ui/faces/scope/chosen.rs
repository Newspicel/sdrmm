use std::cell::RefCell;

use super::{
    colormap::Rgb,
    palette::{CUSTOM_STOPS, DEFAULT_CUSTOM, Palette, PaletteChoice, custom_hexes, parse_custom},
    traces::{AVERAGE_CHOICES, DEFAULT_AVERAGE},
};
use crate::{shell::prefs::Prefs, ui::shell::Shell};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chosen {
    pub choice: PaletteChoice,
    pub custom: [Rgb; CUSTOM_STOPS],
    pub average: u32,
}

impl Default for Chosen {
    fn default() -> Self {
        Self {
            choice: PaletteChoice::default(),
            custom: DEFAULT_CUSTOM,
            average: DEFAULT_AVERAGE,
        }
    }
}

impl Chosen {
    #[must_use]
    pub fn palette(self) -> Palette {
        self.choice.palette(self.custom)
    }

    #[must_use]
    pub fn from_prefs(prefs: &Prefs) -> Self {
        Self {
            choice: prefs
                .scope_palette
                .as_deref()
                .map_or_else(PaletteChoice::default, PaletteChoice::parse),
            custom: parse_custom(&prefs.scope_custom).unwrap_or(DEFAULT_CUSTOM),
            average: prefs
                .scope_average
                .filter(|frames| AVERAGE_CHOICES.contains(frames))
                .unwrap_or(DEFAULT_AVERAGE),
        }
    }

    pub fn write(self, prefs: &mut Prefs) {
        prefs.scope_palette = Some(self.choice.key().to_owned());
        prefs.scope_custom = custom_hexes(&self.custom);
        prefs.scope_average = Some(self.average);
    }
}

thread_local! {
    static CHOSEN: RefCell<Option<Chosen>> = const { RefCell::new(None) };
}

#[must_use]
pub fn chosen(shell: Option<Shell>) -> Chosen {
    if let Some(held) = CHOSEN.with(|chosen| *chosen.borrow()) {
        return held;
    }
    let loaded = shell.map_or_else(Chosen::default, |shell| Chosen::from_prefs(&shell.recall()));
    CHOSEN.with(|chosen| *chosen.borrow_mut() = Some(loaded));
    loaded
}

#[must_use]
pub fn chosen_palette() -> Palette {
    chosen(Shell::current()).palette()
}

pub fn keep(shell: Option<Shell>, change: impl FnOnce(&mut Chosen)) -> Chosen {
    let mut next = chosen(shell);
    change(&mut next);
    CHOSEN.with(|chosen| *chosen.borrow_mut() = Some(next));
    if let Some(shell) = shell {
        shell.remember(|prefs| next.write(prefs));
    }
    next
}

#[cfg(test)]
mod tests {
    use super::{super::colormap::Colormap, *};

    #[test]
    fn choices_survive_a_round_trip_through_the_preferences() {
        let picked = Chosen {
            choice: PaletteChoice::Custom,
            custom: [[1.0, 0.0, 0.0]; CUSTOM_STOPS],
            average: 8,
        };
        let mut prefs = Prefs::default();
        picked.write(&mut prefs);
        assert_eq!(Chosen::from_prefs(&prefs), picked);
        assert_eq!(prefs.scope_palette.as_deref(), Some("custom"));
    }

    #[test]
    fn missing_or_damaged_preferences_fall_back_to_the_defaults() {
        assert_eq!(Chosen::from_prefs(&Prefs::default()), Chosen::default());
        let prefs = Prefs {
            scope_palette: Some(String::from("viridis")),
            scope_custom: vec![String::from("#zzzzzz")],
            scope_average: Some(3),
            ..Prefs::default()
        };
        let read = Chosen::from_prefs(&prefs);
        assert_eq!(read.choice, PaletteChoice::Map(Colormap::Viridis));
        assert_eq!(read.custom, DEFAULT_CUSTOM);
        assert_eq!(read.average, DEFAULT_AVERAGE);
    }

    #[test]
    fn a_kept_choice_is_what_the_next_scope_opens_with() {
        let kept = keep(None, |chosen| chosen.average = 16);
        assert_eq!(kept.average, 16);
        assert_eq!(chosen(None).average, 16);
    }
}
