//! The decoders and encoders this crate compiles in.

use std::sync::Arc;

use sdrmm_decoder::{Decoder, Encoder};

/// Installs [`decoders`] and [`encoders`] as the build's built-ins, once.
pub fn install() {
    sdrmm_decoder::install(decoders, encoders);
}

/// Lists each decoder once: [`decoders`] opens them, and under test each one's positive fixture
/// from its own `fixture` module runs as a test of its own.
macro_rules! built_in {
    ($($module:ident::$constructor:ident),* $(,)?) => {
        /// Every decoder compiled in, in the order the palette lists them; the first is what a
        /// new channel starts as.
        pub fn decoders() -> Vec<Arc<dyn Decoder>> {
            vec![$(Arc::new(crate::$module::$constructor())),*]
        }

        /// Every decoder's positive fixture, by type.
        #[cfg(test)]
        fn positives() -> Vec<(String, fixtures::Positive)> {
            vec![$((
                Decoder::descriptor(&crate::$module::$constructor()).type_id.clone(),
                crate::$module::fixture::$constructor(),
            )),*]
        }

        /// Each decoder emits what it declares and reads what it must from its fixture.
        #[cfg(test)]
        mod positive {
            $(
                #[test]
                fn $constructor() {
                    super::fixtures::check(
                        &crate::$module::$constructor(),
                        &crate::$module::fixture::$constructor(),
                    );
                }
            )*
        }
    };
}

built_in! {
    nfm::nfm_decoder,
    selcall::selcall_decoder,
    am::am_decoder,
    ssb::ssb_decoder,
    wfm::wfm_decoder,
    pocsag::pocsag_decoder,
    flex::flex_decoder,
    ermes::ermes_decoder,
    eot::eot_decoder,
    sarsat::sarsat_decoder,
    tetrapol::tetrapol_decoder,
    adsb::adsb_decoder,
    ais::ais_decoder,
    aprs::aprs_decoder,
    bss::bss_decoder,
    rtty::rtty_decoder,
    morse::morse_decoder,
    cw_skimmer::cw_skimmer_decoder,
    navtex::navtex_decoder,
    acars::acars_decoder,
    atv::atv_decoder,
    sstv::sstv_decoder,
    dab::dab_decoder,
    dvbt::dvbt_decoder,
    datv::datv_decoder,
    drm::drm_decoder,
    dmr::dmr_decoder,
    dstar::dstar_decoder,
    ysf::ysf_decoder,
    nxdn::nxdn_decoder,
    p25::p25_decoder,
    dpmr::dpmr_decoder,
    m17::m17_decoder,
    tetra::tetra_decoder,
    freedv::freedv_decoder,
    weak_signal::ft8_decoder,
    weak_signal::ft4_decoder,
    psk::psk_decoder,
    weak_signal::wspr_decoder,
    ident::ident_decoder,
    radio_clock::radio_clock_decoder,
    gnss::gnss_decoder,
    vor::vor_decoder,
    ils::ils_decoder,
    dsc::dsc_decoder,
    inmarsat_stdc::inmarsat_stdc_decoder,
    inmarsat_aero::inmarsat_aero_decoder,
    vdl2::vdl2_decoder,
    hfdl::hfdl_decoder,
    iridium::iridium_decoder,
    dect::dect_decoder,
    gsm::gsm_decoder,
    apt::apt_decoder,
    lrpt::lrpt_decoder,
    wefax::wefax_decoder,
    same::same_decoder,
    radiosonde::radiosonde_decoder,
    lora::lora_decoder,
    remote_id::remote_id_decoder,
    ble::ble_decoder,
    wifi::wifi_occupancy_decoder,
    ism_survey::ism_survey_decoder,
    ism::ism_decoder,
    wmbus::wmbus_decoder,
    noise_figure::noise_figure_decoder,
    per::per_decoder,
    sid::sid_decoder,
    stanag::stanag_decoder,
}

/// Every encoder compiled in, in the order the palette lists them.
pub fn encoders() -> Vec<Arc<dyn Encoder>> {
    vec![
        Arc::new(crate::nfm::nfm_encoder()),
        Arc::new(crate::am::am_encoder()),
        Arc::new(crate::ssb::ssb_encoder()),
        Arc::new(crate::aprs::aprs_encoder()),
    ]
}

/// Every unit test runs against the built-in decoders, installed once before main.
#[cfg(test)]
#[ctor::ctor(unsafe)]
fn install_for_tests() {
    install();
}

#[cfg(test)]
pub(crate) mod fixtures;
#[cfg(test)]
mod kinds_tests;
#[cfg(test)]
mod signals_tests;
#[cfg(test)]
mod tests;
