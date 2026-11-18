//! The processors this crate brings, as the registry lists them.

use std::sync::Arc;

use crate::{Descriptor, install_built_ins};

/// Every built-in processor type.
#[must_use]
pub fn all() -> Vec<Descriptor> {
    let mut all: Vec<Descriptor> = vec![
        Arc::new(crate::df::descriptor()),
        Arc::new(crate::beamformer::descriptor()),
        Arc::new(crate::spatial_spectrum::descriptor()),
        Arc::new(crate::stitch::descriptor()),
        Arc::new(crate::correlator::descriptor()),
        Arc::new(crate::polarimeter::descriptor()),
        Arc::new(crate::passive_radar::descriptor()),
    ];
    all.extend(probe());
    all
}

#[cfg(feature = "probe")]
fn probe() -> Vec<Descriptor> {
    vec![Arc::new(crate::probe::descriptor())]
}

#[cfg(not(feature = "probe"))]
const fn probe() -> Vec<Descriptor> {
    Vec::new()
}

/// Makes the built-in processors the registry's. Install the decoders first, so ports that take
/// a capability see every kind they bring.
pub fn install() {
    install_built_ins(all);
}
