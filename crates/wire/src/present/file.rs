use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

const SHOWN_IMAGES: [&str; 4] = ["image/jpeg", "image/png", "image/gif", "image/bmp"];
const OPAQUE: &str = "application/octet-stream";
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// A file an event carried, inlined as a data URL so any view can show or save it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PresentedFile {
    /// A name safe to save under.
    pub name: String,
    pub media_type: String,
    /// Whether a view may draw it as a picture.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub image: bool,
    pub url: String,
}

impl PresentedFile {
    #[must_use]
    pub fn inline(name: &str, media_type: &str, bytes: &[u8]) -> Self {
        let image = SHOWN_IMAGES.contains(&media_type);
        let served = if image { media_type } else { OPAQUE };
        Self {
            name: safe_name(name),
            media_type: media_type.to_owned(),
            image,
            url: format!("data:{served};base64,{}", base64(bytes)),
        }
    }
}

fn safe_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_control() || c == '/' || c == '\\' {
                '_'
            } else {
                c
            }
        })
        .collect()
}

fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let word = chunk.iter().enumerate().fold(0u32, |word, (at, &byte)| {
            word | u32::from(byte) << (16 - 8 * at)
        });
        for at in 0..4 {
            if at <= chunk.len() {
                out.push(char::from(ALPHABET[(word >> (18 - 6 * at)) as usize & 63]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_pads_every_tail() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
    }

    #[test]
    fn a_picture_keeps_its_type_and_anything_else_only_downloads() {
        let slide = PresentedFile::inline("slide.png", "image/png", b"png");
        assert!(slide.image);
        assert_eq!(slide.url, "data:image/png;base64,cG5n");

        let page = PresentedFile::inline("../page\n.html", "text/html", b"<b>");
        assert!(!page.image);
        assert_eq!(page.name, ".._page_.html");
        assert!(
            page.url
                .starts_with("data:application/octet-stream;base64,")
        );
    }
}
