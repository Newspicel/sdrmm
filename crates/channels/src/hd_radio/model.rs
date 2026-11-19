use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HdRadioParams {
    /// The audio program to play, 0 for HD1; `None` plays the main program.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<u8>,
}

impl sdrmm_wire::DecoderSettings for HdRadioParams {
    const TYPE_ID: &'static str = "hd_radio";
}

/// NRSC-5 1020s program types.
const PROGRAM_TYPES: [&str; 32] = [
    "",
    "News",
    "Information",
    "Sports",
    "Talk",
    "Rock",
    "Classic Rock",
    "Adult Hits",
    "Soft Rock",
    "Top 40",
    "Country",
    "Oldies",
    "Soft",
    "Nostalgia",
    "Jazz",
    "Classical",
    "Rhythm and Blues",
    "Soft Rhythm and Blues",
    "Foreign Language",
    "Religious Music",
    "Religious Talk",
    "Personality",
    "Public",
    "College",
    "Spanish Talk",
    "Spanish Music",
    "Hip-Hop",
    "",
    "",
    "Weather",
    "Emergency Test",
    "Emergency",
];

/// The program's label: HD1 to HD8, and its type when the station names one.
pub(crate) fn program_label(program: usize, kind: u8) -> String {
    match PROGRAM_TYPES
        .get(usize::from(kind))
        .filter(|name| !name.is_empty())
    {
        Some(name) => format!("HD{} {name}", program + 1),
        None => format!("HD{}", program + 1),
    }
}
