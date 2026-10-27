use std::{
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use bank::{Lane, Pins};
use caps::{GainMode, Plan};
use dongle::{Catalog, Dongle, IN_FLIGHT_SAMPLES, Listing, Radio, Transport};
use sdrmm_device::{
    Capture, CaptureConfig, CaptureRadio, DeviceDriver, DeviceError, RxSink, SdrDevice, lock,
    single_rx_sink,
};
use sdrmm_usb_stream::{Paused, RxStream};
use sdrmm_wire::{
    AgcSetting, BandwidthSetting, Capabilities, DeviceBackend, DeviceInfo, DeviceSettings,
    ExtraValue, GainKind, GainValue,
};

mod bank;
mod caps;
mod convert;
mod dongle;
mod pausing;

pub(crate) const DRIVER_ID: DeviceBackend = DeviceBackend::Rtlsdr;

const DEFAULT_SAMPLE_RATE_HZ: u32 = 2_048_000;
const LANE_SAMPLE_RATE_HZ: u32 = 2_400_000;
const LANE_GAIN_TENTHS: i32 = 297;
pub(crate) const DEFAULT_CENTER_HZ: u32 = 100_000_000;
const PAUSE_TIMEOUT: Duration = Duration::from_secs(1);
const BYTES_PER_SAMPLE: f64 = 2.0;

fn enumerate() -> Result<Vec<Listing>, dongle::Error> {
    Ok(Catalog::scan()?.listings().cloned().collect())
}

#[derive(Default)]
pub struct RtlSdrDriver {
    written: Mutex<Vec<String>>,
}

impl RtlSdrDriver {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl DeviceDriver for RtlSdrDriver {
    fn id(&self) -> DeviceBackend {
        DRIVER_ID
    }

    fn probe(&self) -> Vec<DeviceInfo> {
        match enumerate() {
            Ok(descriptors) => caps::device_infos(&descriptors),
            Err(e) => {
                tracing::warn!("rtlsdr enumerate failed: {e}");
                Vec::new()
            }
        }
    }

    fn open(&self, info: &DeviceInfo) -> Result<Box<dyn SdrDevice>, DeviceError> {
        let catalog = scanned()?;
        let attached = listings(&catalog);
        let index = resolved(&attached, &info.key)?;
        let lane = lane_at(&attached, index);
        Ok(Box::new(RtlSdrDevice::from_dongle(
            catalog.open(index)?,
            lane,
        )?))
    }

    fn write_serial(&self, key: &str, serial: Option<&str>) -> Result<String, DeviceError> {
        let catalog = scanned()?;
        let attached = listings(&catalog);
        let index = resolved(&attached, key)?;
        if let Some(lane) = lane_at(&attached, index) {
            return Err(DeviceError::Unsupported(format!(
                "{} knows its lanes by their serials",
                lane.model.name()
            )));
        }
        let mut written = lock(&self.written);
        let taken: Vec<&str> = attached
            .iter()
            .filter(|listing| listing.index != index)
            .filter_map(|listing| listing.serial.as_deref())
            .chain(written.iter().map(String::as_str))
            .collect();
        let serial = chosen_serial(serial, &taken)?;
        catalog.open(index)?.write_serial(&serial)?;
        tracing::info!(key, serial, "wrote RTL-SDR serial");
        written.push(serial.clone());
        Ok(serial)
    }
}

fn scanned() -> Result<Catalog, DeviceError> {
    Catalog::scan().map_err(|e| DeviceError::Io(format!("rtlsdr enumerate: {e}")))
}

fn listings(catalog: &Catalog) -> Vec<Listing> {
    catalog.listings().cloned().collect()
}

fn lane_at(attached: &[Listing], index: usize) -> Option<Lane> {
    bank::lanes(attached).into_iter().nth(index).flatten()
}

fn resolved(attached: &[Listing], key: &str) -> Result<usize, DeviceError> {
    located(attached, key)
        .map(|listing| listing.index)
        .ok_or_else(|| DeviceError::NotFound(format!("{DRIVER_ID}:{key}")))
}

fn located<'a>(descriptors: &'a [Listing], key: &str) -> Option<&'a Listing> {
    let position = caps::device_infos(descriptors)
        .iter()
        .position(|probed| probed.key == key)?;
    descriptors.get(position)
}

fn chosen_serial(wanted: Option<&str>, taken: &[&str]) -> Result<String, DeviceError> {
    match wanted {
        Some(serial) if !dongle::valid_serial(serial) => Err(DeviceError::Unsupported(
            "a serial is 1 to 16 letters or digits".to_owned(),
        )),
        Some(serial) if taken.contains(&serial) => Err(DeviceError::Unsupported(format!(
            "another RTL-SDR here already has serial {serial}"
        ))),
        Some(serial) => Ok(serial.to_owned()),
        None => random_serial(taken, || {
            getrandom::u32().map_err(|e| DeviceError::Io(format!("random serial: {e}")))
        }),
    }
}

const FACTORY_SERIALS: [&str; 2] = ["00000000", "00000001"];
const SERIAL_DRAWS: usize = 16;

fn random_serial(
    taken: &[&str],
    mut draw: impl FnMut() -> Result<u32, DeviceError>,
) -> Result<String, DeviceError> {
    for _ in 0..SERIAL_DRAWS {
        let serial = format!("{:08}", draw()? % 100_000_000);
        if !taken.contains(&serial.as_str()) && !FACTORY_SERIALS.contains(&serial.as_str()) {
            return Ok(serial);
        }
    }
    Err(DeviceError::Io("no free random serial".to_owned()))
}

pub(crate) struct RtlRadio {
    dongle: Mutex<Dongle>,
}

impl RtlRadio {
    fn new(dongle: Dongle) -> Self {
        Self {
            dongle: Mutex::new(dongle),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Dongle> {
        lock(&self.dongle)
    }
}

impl CaptureRadio for RtlRadio {
    type Stream = RxStream;

    fn arm(&self) -> Result<RxStream, DeviceError> {
        Ok(self.lock().start_stream()?)
    }

    fn disarm(&self) {
        self.lock().set_pauser(None);
    }
}

impl Pins for RtlRadio {
    fn drive(&self, pin: u8, high: bool) -> Result<(), DeviceError> {
        Ok(self.lock().set_pin(pin, high)?)
    }
}

pub struct RtlSdrDevice {
    radio: Arc<RtlRadio>,
    lane: Option<Lane>,
    capabilities: Capabilities,
    settings: DeviceSettings,
    gain_table: Vec<i32>,
    capture: Capture<RtlRadio>,
}

fn loose_start(dongle: &mut Dongle) -> Result<DeviceSettings, DeviceError> {
    dongle.set_sample_rate(DEFAULT_SAMPLE_RATE_HZ)?;
    dongle.set_center(DEFAULT_CENTER_HZ)?;
    dongle.set_auto_gain()?;
    let bias_tee = dongle.bias_tee_at_start();
    dongle.set_bias_tee(bias_tee)?;
    let extra = (!dongle.board().has_upconverter())
        .then(|| ExtraValue {
            name: caps::DIRECT_SAMPLING.to_string(),
            value: dongle.direct_sampling().wire_name().into(),
        })
        .into_iter()
        .collect();
    Ok(DeviceSettings {
        center_hz: dongle.center_hz().map(f64::from),
        sample_rate: Some(f64::from(dongle.sample_rate())),
        ppm: Some(f64::from(dongle.ppm())),
        antenna: Some("RX".to_string()),
        bandwidth: Some(BandwidthSetting::Auto),
        bias_tee: Some(bias_tee),
        agc: Some(AgcSetting::switched(true)),
        extra,
        ..DeviceSettings::default()
    })
}

fn lane_start(dongle: &mut Dongle, lane: &Lane) -> Result<DeviceSettings, DeviceError> {
    dongle.set_dither(false)?;
    dongle.set_sample_rate(LANE_SAMPLE_RATE_HZ)?;
    dongle.set_center(DEFAULT_CENTER_HZ)?;
    dongle.set_manual_gain(LANE_GAIN_TENTHS)?;
    Ok(DeviceSettings {
        center_hz: dongle.center_hz().map(f64::from),
        sample_rate: Some(f64::from(dongle.sample_rate())),
        ppm: Some(f64::from(dongle.ppm())),
        antenna: Some("RX".to_string()),
        bandwidth: Some(BandwidthSetting::Auto),
        bias_tee: lane.model.has_bias_tees().then_some(false),
        agc: Some(AgcSetting::switched(false)),
        gains: vec![GainValue::new(
            GainKind::Tuner,
            f64::from(LANE_GAIN_TENTHS) / 10.0,
        )],
        ..DeviceSettings::default()
    })
}

fn switch_off(lane: &Lane, pins: &dyn Pins) -> Result<(), DeviceError> {
    pins.drive(bank::NOISE_SOURCE_PIN, false)?;
    if lane.model.has_bias_tees() {
        for fed in 0..lane.model.lanes() {
            pins.drive(bank::bias_tee_pin(fed), false)?;
        }
    }
    Ok(())
}

impl RtlSdrDevice {
    fn from_dongle(mut dongle: Dongle, lane: Option<Lane>) -> Result<Self, DeviceError> {
        let capabilities =
            caps::capabilities_of(dongle.board(), lane.as_ref(), dongle.gain_table());
        let gain_table = dongle.gain_table().to_vec();
        tracing::info!(
            tuner = ?dongle.tuner_kind(),
            board = ?dongle.board(),
            gain_steps = gain_table.len(),
            lane = ?lane.as_ref().map(Lane::label),
            "opened rtlsdr device"
        );
        let settings = match &lane {
            Some(lane) => lane_start(&mut dongle, lane)?,
            None => loose_start(&mut dongle)?,
        };
        let radio = Arc::new(RtlRadio::new(dongle));
        if let Some(lane) = lane.as_ref().filter(|lane| lane.controls()) {
            switch_off(lane, radio.as_ref())?;
            let pins: Arc<dyn Pins> = radio.clone();
            bank::hold_control(&lane.id, &pins);
        }
        Ok(Self {
            radio,
            lane,
            capabilities,
            settings,
            gain_table,
            capture: Capture::new(),
        })
    }

    fn pins(&self, lane: &Lane) -> Result<Arc<dyn Pins>, DeviceError> {
        if lane.controls() {
            return Ok(self.radio.clone());
        }
        bank::control(&lane.id).ok_or_else(|| {
            DeviceError::Unsupported(format!("{} lane 1 is not open", lane.model.name()))
        })
    }

    fn switch_bias_tee(&self, lane: &Lane, on: bool) -> Result<(), DeviceError> {
        self.pins(lane)?.drive(bank::bias_tee_pin(lane.lane), on)
    }
}

pub(crate) fn apply_settings<T: Transport>(
    radio: &mut Radio<T>,
    plan: &Plan,
) -> Result<(), dongle::Error> {
    let pause_first = radio.pause_flag();
    let pauser = radio.pauser();
    let bytes_per_second = f64::from(radio.sample_rate()) * BYTES_PER_SAMPLE;
    pausing::apply_pausing(
        &pause_first,
        || apply_to_hardware(radio, plan),
        || match &pauser {
            Some(pauser) => pauser
                .hold(bytes_per_second, PAUSE_TIMEOUT)
                .map(Some)
                .map_err(dongle::Error::from),
            None => Ok(None::<Paused>),
        },
    )
}

fn apply_to_hardware<T: Transport>(
    dongle: &mut Radio<T>,
    plan: &Plan,
) -> Result<(), dongle::Error> {
    if let Some(mode) = plan.direct_sampling {
        dongle.set_direct_sampling(mode)?;
    }
    if let Some(rate) = plan.sample_rate {
        dongle.set_sample_rate(rate)?;
    }
    if let Some(hz) = plan.center_hz {
        dongle.set_center(hz)?;
    }
    if let Some(bandwidth) = plan.bandwidth {
        dongle.set_bandwidth(bandwidth)?;
    }
    if let Some(ppm) = plan.ppm {
        dongle.set_ppm(ppm)?;
    }
    match plan.gain {
        Some(GainMode::Auto) => dongle.set_auto_gain()?,
        Some(GainMode::Manual(tenths)) => dongle.set_manual_gain(tenths)?,
        None => {}
    }
    if let Some(on) = plan.bias_tee {
        dongle.set_bias_tee(on)?;
    }
    Ok(())
}

impl SdrDevice for RtlSdrDevice {
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn settings(&self) -> &DeviceSettings {
        &self.settings
    }

    fn in_flight_samples(&self) -> u64 {
        IN_FLIGHT_SAMPLES
    }

    fn apply(&mut self, settings: &DeviceSettings) -> Result<(), DeviceError> {
        let mut plan = caps::validate(
            settings,
            &self.capabilities,
            &self.settings,
            &self.gain_table,
        )?;
        let on_the_bank = self.lane.is_some().then(|| plan.bias_tee.take()).flatten();
        let (result, center_hz, sample_rate, ppm) = {
            let mut dongle = self.radio.lock();
            let result = apply_settings(&mut dongle, &plan);
            (
                result,
                dongle.center_hz(),
                dongle.sample_rate(),
                dongle.ppm(),
            )
        };
        self.settings.center_hz = center_hz.map(f64::from);
        self.settings.sample_rate = Some(f64::from(sample_rate));
        self.settings.ppm = Some(f64::from(ppm));
        result.map_err(DeviceError::from)?;
        if let Some(on) = on_the_bank
            && let Some(lane) = self.lane.as_ref()
        {
            self.switch_bias_tee(lane, on)?;
        }
        self.settings.merge_from(&plan.applied);
        Ok(())
    }

    fn set_noise_source(&mut self, on: bool) -> Result<(), DeviceError> {
        if !self.lane.as_ref().is_some_and(Lane::controls) {
            return Err(DeviceError::Unsupported(
                "this radio carries no calibration noise source".to_owned(),
            ));
        }
        self.radio.drive(bank::NOISE_SOURCE_PIN, on)
    }

    fn rx_start(&mut self, sinks: Vec<RxSink>) -> Result<(), DeviceError> {
        self.capture.start(
            self.radio.clone(),
            convert::converter(),
            single_rx_sink(sinks)?,
            CaptureConfig::new("sdrmm-rtlsdr-rx", DRIVER_ID.as_str())
                .with_sample_rate(self.settings.sample_rate),
        )
    }

    fn rx_stop(&mut self) {
        self.capture.stop();
    }
}

impl Drop for RtlSdrDevice {
    fn drop(&mut self) {
        self.capture.stop();
        let Some(lane) = self.lane.as_ref().filter(|lane| lane.controls()) else {
            return;
        };
        if let Err(error) = switch_off(lane, self.radio.as_ref()) {
            tracing::warn!(%error, "the bank's noise source and bias tees may still be on");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dongle::{Board, Fake};

    fn dongle(serial: &str, port: u8, hub: Option<u8>) -> Listing {
        Listing {
            index: usize::from(port),
            bus: "001".to_string(),
            address: port,
            manufacturer: None,
            product: None,
            serial: Some(serial.to_string()),
            port_chain: hub.map_or_else(|| vec![port], |hub| vec![hub, port]),
            board: Board::Generic,
            hub: None,
        }
    }

    fn attached_bank() -> Vec<Listing> {
        let mut attached: Vec<Listing> = (0..5)
            .map(|lane| dongle(&(1000 + lane).to_string(), lane as u8 + 1, Some(4)))
            .collect();
        attached.push(dongle("00000123", 9, None));
        attached
    }

    #[test]
    fn every_dongle_of_a_bank_is_a_radio_of_its_own() {
        let infos = caps::device_infos(&attached_bank());
        assert_eq!(infos.len(), 6);
        let banks: Vec<_> = infos
            .iter()
            .filter_map(|info| info.profile.as_ref()?.bank.clone())
            .collect();
        assert_eq!(banks.len(), 5);
        assert!(
            banks
                .iter()
                .all(|bank| bank.id == "001/4" && bank.lanes == 5)
        );
        assert_eq!(
            banks.iter().map(|bank| bank.lane).collect::<Vec<_>>(),
            [0, 1, 2, 3, 4]
        );
        assert_eq!(infos[0].label, "KrakenSDR 001/4 lane 1");
        assert_eq!(infos[0].key, "1000");
        assert_eq!(infos[5].key, "00000123");
        assert!(
            infos[5]
                .profile
                .as_ref()
                .is_some_and(|profile| profile.bank.is_none())
        );
    }

    #[test]
    fn a_lane_declares_the_shared_clock_and_only_the_first_its_noise_source() {
        let infos = caps::device_infos(&attached_bank());
        let profile = |index: usize| infos[index].profile.clone().expect("a profile");
        assert_eq!(profile(0).coherence, sdrmm_wire::Coherence::TimeSync);
        assert_eq!(profile(0).rx_streams, 1);
        assert_eq!(profile(0).noise_source, sdrmm_wire::NoiseSource::Isolated);
        for lane in 1..5 {
            assert_eq!(profile(lane).coherence, sdrmm_wire::Coherence::TimeSync);
            assert_eq!(profile(lane).noise_source, sdrmm_wire::NoiseSource::None);
        }
        assert_eq!(profile(5).coherence, sdrmm_wire::Coherence::None);
    }

    #[test]
    fn ordinary_dongles_are_all_offered() {
        let attached = vec![dongle("00000123", 1, None), dongle("00000124", 2, None)];
        assert_eq!(caps::device_infos(&attached).len(), 2);
    }

    fn draws(values: &[u32]) -> impl FnMut() -> Result<u32, DeviceError> + '_ {
        let mut next = values.iter();
        move || {
            next.next()
                .copied()
                .ok_or(DeviceError::Io("drained".to_owned()))
        }
    }

    #[test]
    fn a_random_serial_is_eight_digits() {
        assert_eq!(random_serial(&[], draws(&[42])).unwrap(), "00000042");
        assert_eq!(random_serial(&[], draws(&[u32::MAX])).unwrap(), "94967295");
    }

    #[test]
    fn a_random_serial_skips_factory_and_taken_ones() {
        assert_eq!(
            random_serial(&["00000042"], draws(&[100_000_000, 1, 42, 7])).unwrap(),
            "00000007"
        );
        assert!(random_serial(&[], draws(&[1; SERIAL_DRAWS])).is_err());
    }

    #[test]
    fn a_chosen_serial_must_be_valid_and_free() {
        assert_eq!(chosen_serial(Some("Roof"), &["00000001"]).unwrap(), "Roof");
        let random = chosen_serial(None, &[]).unwrap();
        assert!(
            dongle::valid_serial(&random) && random.len() == 8,
            "{random}"
        );
        for wanted in ["00000001", "has space", ""] {
            assert!(matches!(
                chosen_serial(Some(wanted), &["00000001"]),
                Err(DeviceError::Unsupported(_))
            ));
        }
    }

    #[test]
    fn a_serialless_dongle_is_found_by_its_location() {
        let mut bare = dongle("x", 3, None);
        bare.serial = None;
        let attached = vec![dongle("00000123", 1, None), bare];
        assert_eq!(located(&attached, "001/3").map(|l| l.index), Some(3));
        assert_eq!(located(&attached, "00000123").map(|l| l.index), Some(1));
        assert!(located(&attached, "001/9").is_none());
    }

    fn numbered(mut listed: Vec<Listing>) -> Vec<Listing> {
        for (index, listing) in listed.iter_mut().enumerate() {
            listing.index = index;
        }
        listed
    }

    #[test]
    fn a_dongle_is_opened_at_its_place_in_the_scan_that_found_it() {
        let attached = numbered(attached_bank());
        assert_eq!(resolved(&attached, "00000123").unwrap(), 5);
        assert_eq!(resolved(&attached, "1002").unwrap(), 2);
        assert_eq!(lane_at(&attached, 2).map(|lane| lane.lane), Some(2));
        assert!(lane_at(&attached, 5).is_none());
        let alone = numbered(vec![dongle("00000123", 9, None)]);
        assert_eq!(
            resolved(&alone, "00000123").unwrap(),
            0,
            "an index only means anything in the scan it came from"
        );
    }

    fn tuned(hz: u32) -> Plan {
        Plan {
            center_hz: Some(hz),
            ..Plan::default()
        }
    }

    #[test]
    fn a_stall_while_applying_remembers_to_pause_the_stream() {
        let fake = Fake::default();
        let mut radio = Radio::start(fake.clone(), Board::Generic).expect("start");
        assert!(apply_settings(&mut radio, &tuned(100_000_000)).is_ok());
        assert!(!radio.pauses_control());
        fake.fail_writes(0x0120, 0x0011);
        assert!(
            apply_settings(&mut radio, &tuned(101_000_000)).is_err_and(|error| error.is_stall())
        );
        assert!(
            radio.pauses_control(),
            "settings go through the pausing wrapper, so the stall is remembered"
        );
    }

    #[test]
    fn driver_id_is_the_wire_id() {
        assert_eq!(RtlSdrDriver::new().id(), DeviceBackend::Rtlsdr);
    }
}
