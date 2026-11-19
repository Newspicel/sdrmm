//! LRIT and HRIT (CGMS 03): files carried as CCSDS transport files on virtual channels, their
//! header records, and NOAA's Rice-coded image lines. GOES HRIT and GK-2A LRIT share it.

mod coarse;
pub mod header;
mod link;
mod mosaic;
mod rice;
mod session;
mod transmit;

pub use header::{CdsTime, HeaderWriter, Headers, ImageStructure, Noaa, Primary, Rice, SegmentId};
pub use link::{LinkConfig, LinkError, LinkStats, XritLink};
pub use mosaic::{Canvas, ImageSegment, Mosaic};
pub use rice::{MSB, NN, RiceError, RiceLine, RiceParams};
pub use session::{SessionAssembler, SessionStats, TRANSPORT_HEADER, TransportFile};
pub use transmit::{file_packets, modulate, rice_encode, rice_image_packets};

#[cfg(test)]
mod tests;
