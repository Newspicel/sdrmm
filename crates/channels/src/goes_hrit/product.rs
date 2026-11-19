use super::GoesPicture;

/// What an ABI file name says about its image: `OR_ABI-L2-CMIPF-M6C13_G19_s...` is GOES-19's
/// full disk in band 13.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Product {
    pub satellite: Option<String>,
    pub region: Region,
    pub band: Option<u8>,
    /// The ABI product code without its region, such as `CMIP` or `ACHT`.
    pub code: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    FullDisk,
    Conus,
    Mesoscale(u8),
    Other,
}

impl Region {
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::FullDisk => "Full disk".to_owned(),
            Self::Conus => "CONUS".to_owned(),
            Self::Mesoscale(number) => format!("Mesoscale {number}"),
            Self::Other => "Image".to_owned(),
        }
    }
}

impl Product {
    #[must_use]
    pub fn parse(name: &str) -> Self {
        let satellite = name.split(['_', '.', '-']).find_map(|part| {
            let number = part.strip_prefix('G')?;
            (number.len() == 2 && number.bytes().all(|b| b.is_ascii_digit()))
                .then(|| format!("GOES-{number}"))
        });
        let abi = name.split('_').find_map(|part| part.strip_prefix("ABI-"));
        let (code, region, band) = abi.map_or((None, Region::Other, None), parse_abi);
        Self {
            satellite,
            region,
            band,
            code,
        }
    }

    /// The product as a picture title: `Full disk`, or `ACHT full disk` for a level-2 field.
    #[must_use]
    pub fn label(&self) -> String {
        match self.code.as_deref() {
            None | Some("CMIP" | "Rad") => self.region.label(),
            Some(code) => format!("{code} {}", self.region.label().to_lowercase()),
        }
    }

    #[must_use]
    pub fn wanted(&self, picture: GoesPicture) -> bool {
        match picture {
            GoesPicture::All => true,
            GoesPicture::FullDisk => self.region == Region::FullDisk,
            GoesPicture::Mesoscale => matches!(self.region, Region::Mesoscale(_)),
        }
    }
}

fn parse_abi(abi: &str) -> (Option<String>, Region, Option<u8>) {
    let mut parts = abi.split('-');
    let _level = parts.next();
    let Some(product) = parts.next() else {
        return (None, Region::Other, None);
    };
    let mode = parts.next().unwrap_or_default();
    let band = mode
        .split_once('C')
        .and_then(|(_, band)| band.get(..2))
        .and_then(|band| band.parse().ok());
    let (code, region) = if let Some(code) = product.strip_suffix('F') {
        (code, Region::FullDisk)
    } else if let Some(code) = product.strip_suffix('C') {
        (code, Region::Conus)
    } else if let Some((code, number)) = product.split_at_checked(product.len().saturating_sub(2))
        && let Some(number) = number.strip_prefix('M')
        && let Ok(number) = number.parse()
    {
        (code, Region::Mesoscale(number))
    } else {
        (product, Region::Other)
    };
    (Some(code.to_owned()), region, band)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abi_names_give_satellite_region_and_band() {
        let full = Product::parse("OR_ABI-L2-CMIPF-M6C13_G19_s20251772230205_e2025.lrit");
        assert_eq!(full.satellite.as_deref(), Some("GOES-19"));
        assert_eq!(full.region, Region::FullDisk);
        assert_eq!(full.band, Some(13));
        assert_eq!(full.label(), "Full disk");
        let meso = Product::parse("OR_ABI-L2-CMIPM1-M6C02_G19_s20251772237251.lrit");
        assert_eq!(meso.region, Region::Mesoscale(1));
        assert_eq!(meso.band, Some(2));
        assert!(meso.wanted(GoesPicture::Mesoscale));
        assert!(!meso.wanted(GoesPicture::FullDisk));
        let field = Product::parse("OR_ABI-L2-ACHTF-M6_G19_s20251772200205.lrit");
        assert_eq!((field.region, field.band), (Region::FullDisk, None));
        assert_eq!(field.label(), "ACHT full disk");
        let other = Product::parse("20250626230001-atl48per_latest.lrit");
        assert_eq!(other.region, Region::Other);
        assert!(other.wanted(GoesPicture::All));
    }
}
