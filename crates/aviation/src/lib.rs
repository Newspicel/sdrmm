//! The aviation datalink layers above the radio: ACARS blocks and the applications they carry,
//! ARINC 622 and CPDLC. VDL2, HFDL, Iridium, Inmarsat Aero and ACARS itself share them, and so
//! can a plugin.

pub mod acars;
pub mod cpdlc;
