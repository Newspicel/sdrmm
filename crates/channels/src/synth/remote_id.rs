use num_complex::Complex;
use sdrmm_modem::{ble::transmit, spread::PnError};
use sdrmm_wire::RemoteIdMessage;

pub use sdrmm_modem::{
    ble::BlePhy,
    wifi::{
        WifiPhy,
        transmit::{dsss, ofdm},
    },
};

use crate::remote_id::odid::{self, APP_CODE, SERVICE_UUID};

pub use crate::remote_id::{odid::encode, wifi::build};

const ADV_NONCONN_IND: u8 = 0x02;
const ADV_EXT_IND: u8 = 0x07;
const TX_ADD_RANDOM: u8 = 0x40;
const FLAG_ADDRESS: u8 = 0x01;
const FLAG_ADI: u8 = 0x08;
const FLAG_AUX_PTR: u8 = 0x10;
const ADI: [u8; 2] = [0x75, 0x0E];
const AUX_OFFSET_UNITS: u16 = 40;

fn service_data(counter: u8, payload: &[u8]) -> Vec<u8> {
    let uuid = SERVICE_UUID.to_le_bytes();
    let mut out = vec![
        (payload.len() + 5) as u8,
        0x16,
        uuid[0],
        uuid[1],
        APP_CODE,
        counter,
    ];
    out.extend_from_slice(payload);
    out
}

#[must_use]
pub fn legacy_pdu(address: [u8; 6], counter: u8, message: &RemoteIdMessage) -> Vec<u8> {
    let data = service_data(counter, &odid::encode::message(message));
    let mut pdu = vec![ADV_NONCONN_IND | TX_ADD_RANDOM, (6 + data.len()) as u8];
    pdu.extend(address.iter().rev());
    pdu.extend_from_slice(&data);
    pdu
}

fn extended(header: &[u8], data: &[u8]) -> Vec<u8> {
    let mut pdu = vec![
        ADV_EXT_IND | TX_ADD_RANDOM,
        (1 + header.len() + data.len()) as u8,
        header.len() as u8,
    ];
    pdu.extend_from_slice(header);
    pdu.extend_from_slice(data);
    pdu
}

#[must_use]
pub fn aux_adv_pdu(address: [u8; 6], counter: u8, messages: &[RemoteIdMessage]) -> Vec<u8> {
    let mut header = vec![FLAG_ADDRESS | FLAG_ADI];
    header.extend(address.iter().rev());
    header.extend_from_slice(&ADI);
    extended(
        &header,
        &service_data(counter, &odid::encode::pack(messages)),
    )
}

fn aux_phy(phy: BlePhy) -> u8 {
    match phy {
        BlePhy::Le1m => 0,
        BlePhy::CodedS8 | BlePhy::CodedS2 => 2,
    }
}

#[must_use]
pub fn ext_adv_pdu(aux_channel_index: u8, aux: BlePhy) -> Vec<u8> {
    let pointer = u32::from(aux_channel_index & 0x3F)
        | u32::from(AUX_OFFSET_UNITS) << 8
        | u32::from(aux_phy(aux)) << 21;
    let mut header = vec![FLAG_ADI | FLAG_AUX_PTR];
    header.extend_from_slice(&ADI);
    header.extend_from_slice(&pointer.to_le_bytes()[..3]);
    extended(&header, &[])
}

#[must_use]
pub fn bluetooth(pdu: &[u8], rf_channel: u8, phy: BlePhy, rate: f64) -> Vec<Complex<f32>> {
    transmit::packet(pdu, sdrmm_modem::ble::channel_index(rf_channel), phy, rate)
}

const SCENE_ADDRESS: [u8; 6] = [0xD4, 0x5A, 0x21, 0x0C, 0x7E, 0x19];
const SCENE_GAP_S: f64 = 0.02;

#[must_use]
pub fn demo_drone() -> Vec<RemoteIdMessage> {
    use sdrmm_wire::{
        EuCategory, EuClassification, HeightReference, OperatorLocationType, RemoteIdLocation,
        RemoteIdSystem, UaStatus, UaType, UasIdType,
    };
    vec![
        RemoteIdMessage::BasicId {
            id_type: UasIdType::SerialNumber,
            ua_type: UaType::Rotorcraft,
            uas_id: "1581F5FJD239C00DW22E".to_owned(),
        },
        RemoteIdMessage::Location(RemoteIdLocation {
            status: UaStatus::Airborne,
            lat: Some(52.516_3),
            lon: Some(13.377_7),
            track_deg: Some(74.0),
            speed_mps: Some(6.5),
            vertical_speed_mps: Some(0.5),
            pressure_altitude_m: Some(118.5),
            geodetic_altitude_m: Some(156.0),
            height_m: Some(84.0),
            height_reference: HeightReference::Takeoff,
            horizontal_accuracy_m: Some(3.0),
            vertical_accuracy_m: Some(10.0),
            pressure_accuracy_m: None,
            speed_accuracy_mps: Some(1.0),
            seconds_after_hour: Some(1_204.5),
            timestamp_accuracy_s: Some(0.1),
        }),
        RemoteIdMessage::SelfId {
            description_type: 0,
            text: "Roof survey".to_owned(),
        },
        RemoteIdMessage::System(RemoteIdSystem {
            operator_location_type: OperatorLocationType::LiveGnss,
            operator_lat: Some(52.515_6),
            operator_lon: Some(13.376_1),
            operator_altitude_m: Some(71.0),
            area_count: 1,
            area_radius_m: 0,
            area_ceiling_m: None,
            area_floor_m: None,
            classification: Some(EuClassification {
                category: EuCategory::Open,
                class: Some(1),
            }),
            timestamp: Some(1_791_460_800),
        }),
        RemoteIdMessage::OperatorId {
            id_type: 0,
            operator_id: "DEUx2b9q1k4m7n3p".to_owned(),
        },
    ]
}

fn gap(rate: f64) -> Vec<Complex<f32>> {
    vec![Complex::new(0.0, 0.0); (SCENE_GAP_S * rate) as usize]
}

#[must_use]
pub fn bluetooth_scene(rate: f64) -> Vec<Complex<f32>> {
    let mut iq = gap(rate);
    for (counter, message) in demo_drone().iter().enumerate() {
        let pdu = legacy_pdu(SCENE_ADDRESS, counter as u8, message);
        iq.extend(bluetooth(&pdu, 0, BlePhy::Le1m, rate));
        iq.extend(gap(rate));
    }
    iq.extend(bluetooth(
        &ext_adv_pdu(8, BlePhy::CodedS8),
        0,
        BlePhy::CodedS8,
        rate,
    ));
    iq.extend(gap(rate));
    iq
}

pub fn wifi_scene() -> Result<Vec<Complex<f32>>, PnError> {
    let rate = sdrmm_wire::RemoteIdLink::Wifi.input_rate_hz();
    let pack = encode::pack(&demo_drone());
    let mut iq = gap(rate);
    iq.extend(ofdm(
        &build::beacon(SCENE_ADDRESS, "RID-DW22E", 1, &pack),
        6,
    ));
    iq.extend(gap(rate));
    iq.extend(dsss(
        &build::nan(SCENE_ADDRESS, 2, &pack),
        WifiPhy::Dsss1m,
        false,
    )?);
    iq.extend(gap(rate));
    Ok(iq)
}
