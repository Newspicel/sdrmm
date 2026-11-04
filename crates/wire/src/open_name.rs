//! Names drawn from an open set: the built-in ones are constants, a plugin may add its own.

/// A string newtype serialized as the bare string, with constants for the built-in names.
macro_rules! open_name {
    ($(#[$meta:meta])* $name:ident { $($(#[$item:meta])* $const:ident = $tag:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(
            Clone, PartialEq, Eq, Hash, PartialOrd, Ord,
            serde::Serialize, serde::Deserialize, utoipa::ToSchema,
        )]
        #[serde(transparent)]
        #[schema(value_type = String)]
        pub struct $name(std::borrow::Cow<'static, str>);

        impl $name {
            $($(#[$item])* pub const $const: Self = Self::named($tag);)+

            /// Every built-in name, in display order.
            pub const BUILT_IN: &'static [Self] = &[$(Self::$const),+];

            #[must_use]
            pub const fn named(tag: &'static str) -> Self {
                Self(std::borrow::Cow::Borrowed(tag))
            }

            #[must_use]
            pub fn new(tag: impl Into<String>) -> Self {
                Self(std::borrow::Cow::Owned(tag.into()))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}({})", stringify!($name), self.as_str())
            }
        }

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.as_str() == other
            }
        }

        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.as_str() == *other
            }
        }
    };
}

pub(crate) use open_name;

#[cfg(test)]
mod tests {
    use crate::{DecoderFamily, Symbol};

    #[test]
    fn a_built_in_name_keeps_its_snake_case_form() {
        let json = serde_json::to_string(&DecoderFamily::ANALOG_VOICE).expect("family");
        assert_eq!(json, "\"analog_voice\"");
        assert_eq!(
            serde_json::to_string(&Symbol::PLANE).expect("symbol"),
            "\"plane\""
        );
        let back: DecoderFamily = serde_json::from_str("\"aviation\"").expect("parses");
        assert_eq!(back, DecoderFamily::AVIATION);
        assert_eq!(DecoderFamily::default(), DecoderFamily::UTILITY);
    }

    #[test]
    fn a_plugin_may_name_its_own() {
        let family: DecoderFamily = serde_json::from_str("\"space_weather\"").expect("parses");
        assert_eq!(family, "space_weather");
        assert!(!DecoderFamily::BUILT_IN.contains(&family));
        let symbol = Symbol::new("kite");
        assert_eq!(serde_json::to_string(&symbol).expect("symbol"), "\"kite\"");
        assert_eq!(Symbol::BUILT_IN.len(), 15);
    }
}
