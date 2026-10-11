use serde_json::Value;

use super::geo::TileId;

const DEFAULT_MAX_ZOOM: u8 = 14;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StyleSource {
    Vector { template: String, max_zoom: u8 },
    TileJson(String),
    Raster(String),
}

fn first_tile(source: &Value) -> Option<String> {
    source
        .get("tiles")?
        .as_array()?
        .first()?
        .as_str()
        .map(str::to_owned)
}

fn vector(source: &Value) -> Option<StyleSource> {
    if let Some(template) = first_tile(source) {
        let max_zoom = source
            .get("maxzoom")
            .and_then(Value::as_u64)
            .and_then(|zoom| u8::try_from(zoom).ok())
            .unwrap_or(DEFAULT_MAX_ZOOM);
        return Some(StyleSource::Vector { template, max_zoom });
    }
    let url = source.get("url")?.as_str()?;
    url.starts_with("http")
        .then(|| StyleSource::TileJson(url.to_owned()))
}

#[must_use]
pub fn style_source(style: &Value) -> Option<StyleSource> {
    let sources = style.get("sources")?.as_object()?;
    let of_type = |kind: &'static str| {
        sources
            .values()
            .filter(move |source| source.get("type").and_then(Value::as_str) == Some(kind))
    };
    of_type("vector").find_map(vector).or_else(|| {
        of_type("raster")
            .find_map(first_tile)
            .map(StyleSource::Raster)
    })
}

#[must_use]
pub fn quadkey(id: TileId) -> String {
    (1..=id.z)
        .rev()
        .map(|level| {
            let mask = 1u32 << (level - 1);
            let digit = u8::from(id.x & mask != 0) + 2 * u8::from(id.y & mask != 0);
            char::from(b'0' + digit)
        })
        .collect()
}

#[must_use]
pub fn tile_url(template: &str, id: TileId) -> String {
    template
        .replace("{z}", &id.z.to_string())
        .replace("{x}", &id.x.to_string())
        .replace("{y}", &id.y.to_string())
        .replace("{quadkey}", &quadkey(id))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_style_names_its_vector_tiles_first() {
        let style = json!({
            "sources": {
                "hills": {"type": "raster", "tiles": ["https://r/{z}/{x}/{y}.png"]},
                "base": {"type": "vector", "url": "https://v/tiles.json"}
            }
        });
        assert_eq!(
            style_source(&style),
            Some(StyleSource::TileJson("https://v/tiles.json".to_owned()))
        );
        let inline = json!({
            "sources": {"base": {"type": "vector", "tiles": ["https://v/{z}/{x}/{y}.pbf"], "maxzoom": 12}}
        });
        assert_eq!(
            style_source(&inline),
            Some(StyleSource::Vector {
                template: "https://v/{z}/{x}/{y}.pbf".to_owned(),
                max_zoom: 12
            })
        );
    }

    #[test]
    fn a_raster_only_style_is_drawn_as_pictures_and_an_empty_one_is_refused() {
        let style =
            json!({"sources": {"sat": {"type": "raster", "tiles": ["https://s/{z}/{x}/{y}.jpg"]}}});
        assert_eq!(
            style_source(&style),
            Some(StyleSource::Raster("https://s/{z}/{x}/{y}.jpg".to_owned()))
        );
        assert_eq!(style_source(&json!({"sources": {}})), None);
        assert_eq!(style_source(&json!({"version": 8})), None);
    }

    #[test]
    fn a_tile_url_fills_every_placeholder() {
        let id = TileId { z: 3, x: 3, y: 5 };
        assert_eq!(quadkey(id), "213");
        assert_eq!(quadkey(TileId { z: 0, x: 0, y: 0 }), "");
        assert_eq!(
            tile_url("https://t/{z}/{x}/{y}.png", id),
            "https://t/3/3/5.png"
        );
        assert_eq!(
            tile_url("https://t/{quadkey}.jpeg", id),
            "https://t/213.jpeg"
        );
    }
}
