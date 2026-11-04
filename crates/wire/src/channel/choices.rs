use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ParamChoice {
    pub name: String,
    pub options: Vec<ParamOption>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ParamOption {
    pub value: serde_json::Value,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

pub trait SettingChoice: Copy + Serialize + 'static {
    fn all() -> &'static [Self];
    fn choice_label(self) -> &'static str;
    fn choice_title(self) -> Option<&'static str> {
        None
    }
}

#[must_use]
pub fn option(value: serde_json::Value, label: String, title: Option<&str>) -> ParamOption {
    ParamOption {
        value,
        label,
        title: title.map(str::to_owned),
    }
}

#[must_use]
pub fn of<T: SettingChoice>(value: T) -> ParamOption {
    option(
        serde_json::to_value(value).unwrap_or_default(),
        value.choice_label().to_owned(),
        value.choice_title(),
    )
}

#[macro_export]
macro_rules! labelled {
    ($($ty:ty { $($variant:ident => $label:literal $(: $title:literal)?),+ $(,)? })+) => {
        $(
            impl $crate::SettingChoice for $ty {
                fn all() -> &'static [Self] {
                    &[$(<$ty>::$variant),+]
                }

                fn choice_label(self) -> &'static str {
                    match self {
                        $(<$ty>::$variant => $label),+
                    }
                }

                fn choice_title(self) -> Option<&'static str> {
                    match self {
                        $(<$ty>::$variant => $crate::labelled!(@title $($title)?)),+
                    }
                }
            }
        )+
    };
    (@title $title:literal) => { Some($title) };
    (@title) => { None };
}

#[macro_export]
macro_rules! named {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl $crate::SettingChoice for $ty {
                fn all() -> &'static [Self] {
                    &<$ty>::ALL
                }

                fn choice_label(self) -> &'static str {
                    self.label()
                }
            }
        )+
    };
}
