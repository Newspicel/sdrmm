mod acurite;
mod bresser;
mod fineoffset;
mod lacrosse;
mod nexus;
mod oregon;
mod remote;
mod thermometer;
mod tpms;

use super::device::Device;

pub(super) const ALL: &[Device] = &[
    remote::GENERIC,
    remote::HONEYWELL,
    remote::KERUI,
    acurite::TXR,
    acurite::TX606,
    acurite::TXC609,
    lacrosse::TX141,
    lacrosse::TX29,
    lacrosse::TX35,
    nexus::RUBICSON,
    nexus::ALECTO,
    nexus::NEXUS,
    nexus::PROLOGUE,
    oregon::OREGON,
    fineoffset::WH2,
    fineoffset::WHX080,
    fineoffset::WH24,
    fineoffset::WH31E,
    thermometer::F007TH,
    thermometer::TP12,
    tpms::SCHRADER,
    bresser::FIVE_IN_ONE,
    bresser::SIX_IN_ONE,
];
