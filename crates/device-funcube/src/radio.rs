use crate::{
    error::{Error, Result},
    protocol::{self, Command, IfFilter, MAX_IF_GAIN_DB, Report, RfFilter},
};

pub(crate) const MAX_PPM: f64 = 200.0;
const DEFAULT_HZ: u32 = 100_000_000;

pub(crate) trait Link: Send {
    fn exchange(&mut self, request: &Report) -> Result<Report>;
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Config {
    pub(crate) asked_hz: u32,
    pub(crate) tuned_hz: u32,
    pub(crate) ppm: f64,
    pub(crate) lna: bool,
    pub(crate) mixer: bool,
    pub(crate) if_gain_db: u8,
    pub(crate) if_filter: IfFilter,
    pub(crate) rf_filter: Option<RfFilter>,
    pub(crate) bias_tee: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            asked_hz: DEFAULT_HZ,
            tuned_hz: 0,
            ppm: 0.0,
            lna: true,
            mixer: true,
            if_gain_db: 0,
            if_filter: IfFilter::Khz200,
            rf_filter: None,
            bias_tee: false,
        }
    }
}

impl Config {
    pub(crate) fn center_hz(&self) -> f64 {
        f64::from(self.tuned_hz) * (1.0 + self.ppm * 1e-6)
    }
}

pub(crate) fn tuned_hz(asked_hz: u32, ppm: f64) -> u32 {
    (f64::from(asked_hz) / (1.0 + ppm * 1e-6)).round() as u32
}

pub(crate) fn validate_ppm(ppm: f64) -> Result<()> {
    if ppm.is_finite() && (-MAX_PPM..=MAX_PPM).contains(&ppm) {
        return Ok(());
    }
    Err(Error::invalid_config(
        "ppm",
        "the correction stops at 200 ppm",
    ))
}

pub(crate) struct FunCube<L> {
    link: L,
    config: Config,
    firmware: String,
}

impl<L: Link> FunCube<L> {
    pub(crate) fn open(link: L) -> Result<Self> {
        let mut radio = Self {
            link,
            config: Config::default(),
            firmware: String::new(),
        };
        let reply = radio.call(Command::Query, &[])?;
        radio.firmware = protocol::decode_firmware(&reply[2..])?;
        let defaults = Config::default();
        radio.set_lna(defaults.lna)?;
        radio.set_mixer(defaults.mixer)?;
        radio.set_if_gain_db(defaults.if_gain_db)?;
        radio.set_if_filter(defaults.if_filter)?;
        radio.set_bias_tee(defaults.bias_tee)?;
        radio.tune()?;
        Ok(radio)
    }

    pub(crate) fn config(&self) -> &Config {
        &self.config
    }

    pub(crate) fn firmware(&self) -> &str {
        &self.firmware
    }

    fn call(&mut self, command: Command, payload: &[u8]) -> Result<Report> {
        let reply = self.link.exchange(&protocol::request(command, payload))?;
        protocol::payload(command, &reply)?;
        Ok(reply)
    }

    #[cfg(test)]
    fn read_u8(&mut self, command: Command) -> Result<u8> {
        let reply = self.call(command, &[])?;
        protocol::decode_u8(command, &reply[2..])
    }

    pub(crate) fn set_frequency_hz(&mut self, frequency_hz: u32) -> Result<()> {
        if !protocol::in_band(frequency_hz) {
            return Err(Error::invalid_config(
                "frequency",
                "a Pro+ tunes 150 kHz to 260 MHz and 410 MHz to 2.05 GHz",
            ));
        }
        self.config.asked_hz = frequency_hz;
        self.retune()
    }

    pub(crate) fn set_ppm(&mut self, ppm: f64) -> Result<()> {
        validate_ppm(ppm)?;
        self.config.ppm = ppm;
        self.retune()
    }

    fn retune(&mut self) -> Result<()> {
        if tuned_hz(self.config.asked_hz, self.config.ppm) == self.config.tuned_hz {
            return Ok(());
        }
        self.tune()
    }

    fn tune(&mut self) -> Result<()> {
        let hz = tuned_hz(self.config.asked_hz, self.config.ppm);
        let reply = self.call(Command::SetFrequencyHz, &hz.to_le_bytes())?;
        self.config.tuned_hz = protocol::decode_u32(Command::SetFrequencyHz, &reply[2..])?;
        if let Some(filter) = self.config.rf_filter {
            self.call(Command::SetRfFilter, &[filter as u8])?;
        }
        Ok(())
    }

    pub(crate) fn set_lna(&mut self, on: bool) -> Result<()> {
        self.call(Command::SetLna, &[u8::from(on)])?;
        self.config.lna = on;
        Ok(())
    }

    pub(crate) fn set_mixer(&mut self, on: bool) -> Result<()> {
        self.call(Command::SetMixer, &[u8::from(on)])?;
        self.config.mixer = on;
        Ok(())
    }

    pub(crate) fn set_if_gain_db(&mut self, db: u8) -> Result<()> {
        if db > MAX_IF_GAIN_DB {
            return Err(Error::invalid_config(
                "IF gain",
                "the IF gain stops at 59 dB",
            ));
        }
        self.call(Command::SetIfGain, &[db])?;
        self.config.if_gain_db = db;
        Ok(())
    }

    pub(crate) fn set_if_filter(&mut self, filter: IfFilter) -> Result<()> {
        self.call(Command::SetIfFilter, &[filter as u8])?;
        self.config.if_filter = filter;
        Ok(())
    }

    pub(crate) fn set_bias_tee(&mut self, on: bool) -> Result<()> {
        self.call(Command::SetBiasTee, &[u8::from(on)])?;
        self.config.bias_tee = on;
        Ok(())
    }

    pub(crate) fn set_rf_filter(&mut self, filter: Option<RfFilter>) -> Result<()> {
        let previous = std::mem::replace(&mut self.config.rf_filter, filter);
        let written = match filter {
            Some(filter) => self.call(Command::SetRfFilter, &[filter as u8]).map(drop),
            None => self.tune(),
        };
        if written.is_err() {
            self.config.rf_filter = previous;
        }
        written
    }

    #[cfg(test)]
    pub(crate) fn active_rf_filter(&mut self) -> Result<RfFilter> {
        let index = self.read_u8(Command::GetRfFilter)?;
        RfFilter::from_index(index).ok_or(Error::protocol(
            Command::GetRfFilter.name(),
            "the firmware named an RF filter it does not have",
        ))
    }

    #[cfg(test)]
    pub(crate) fn read_back(&mut self) -> Result<Config> {
        let reply = self.call(Command::GetFrequencyHz, &[])?;
        let tuned_hz = protocol::decode_u32(Command::GetFrequencyHz, &reply[2..])?;
        let if_filter = IfFilter::from_index(self.read_u8(Command::GetIfFilter)?)
            .ok_or(Error::protocol("read IF filter", "unknown filter"))?;
        Ok(Config {
            tuned_hz,
            lna: self.read_u8(Command::GetLna)? != 0,
            mixer: self.read_u8(Command::GetMixer)? != 0,
            if_gain_db: self.read_u8(Command::GetIfGain)?,
            if_filter,
            bias_tee: self.read_u8(Command::GetBiasTee)? != 0,
            ..self.config.clone()
        })
    }
}

#[cfg(test)]
pub(crate) mod fake {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::protocol::REPORT_LEN;

    #[derive(Debug, Default)]
    pub(crate) struct State {
        pub(crate) sent: Vec<Report>,
        pub(crate) refuse: Option<Command>,
        pub(crate) bootloader: bool,
        pub(crate) frequency_hz: u32,
        pub(crate) rf_filter: u8,
        pub(crate) settings: Vec<u8>,
    }

    #[derive(Clone, Debug, Default)]
    pub(crate) struct FakeLink(pub(crate) Arc<Mutex<State>>);

    impl FakeLink {
        pub(crate) fn sent(&self) -> Vec<(u8, Vec<u8>)> {
            sdrmm_device::lock(&self.0)
                .sent
                .iter()
                .map(|report| (report[0], report[1..5].to_vec()))
                .collect()
        }

        pub(crate) fn clear(&self) {
            sdrmm_device::lock(&self.0).sent.clear();
        }
    }

    fn auto_filter(hz: u32) -> u8 {
        match hz {
            0..=4_000_000 => 0,
            4_000_001..=75_000_000 => 4,
            75_000_001..=125_000_000 => 5,
            125_000_001..=250_000_000 => 6,
            250_000_001..=875_000_000 => 8,
            _ => 10,
        }
    }

    impl Link for FakeLink {
        fn exchange(&mut self, request: &Report) -> Result<Report> {
            let mut state = sdrmm_device::lock(&self.0);
            state.settings.resize(256, 0);
            state.sent.push(*request);
            let mut reply = [0; REPORT_LEN];
            reply[0] = request[0];
            if state
                .refuse
                .is_some_and(|command| command as u8 == request[0])
            {
                return Ok(reply);
            }
            reply[1] = 1;
            let value = request[1];
            match request[0] {
                1 => {
                    let text: &[u8] = if state.bootloader {
                        b"FCDBL 2.0"
                    } else {
                        b"FCDAPP 20.03 Brd 2.0 No blk"
                    };
                    reply[2..2 + text.len()].copy_from_slice(text);
                }
                101 => {
                    let hz = u32::from_le_bytes([request[1], request[2], request[3], request[4]]);
                    state.frequency_hz = hz;
                    state.rf_filter = auto_filter(hz);
                    reply[2..6].copy_from_slice(&hz.to_le_bytes());
                }
                102 => reply[2..6].copy_from_slice(&state.frequency_hz.to_le_bytes()),
                113 => state.rf_filter = value,
                153 => reply[2] = state.rf_filter,
                110..=126 => state.settings[usize::from(request[0])] = value,
                150..=166 => reply[2] = state.settings[usize::from(request[0]) - 40],
                _ => reply[1] = 0,
            }
            Ok(reply)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{fake::FakeLink, *};

    fn opened() -> (FunCube<FakeLink>, FakeLink) {
        let link = FakeLink::default();
        let radio = FunCube::open(link.clone()).expect("open");
        link.clear();
        (radio, link)
    }

    #[test]
    fn opening_reads_the_firmware_and_writes_the_power_on_state() {
        let link = FakeLink::default();
        let mut radio = FunCube::open(link.clone()).expect("open");
        assert_eq!(radio.firmware(), "FCDAPP 20.03 Brd 2.0 No blk");
        let commands: Vec<u8> = link.sent().iter().map(|(command, _)| *command).collect();
        assert_eq!(commands, vec![1, 110, 114, 117, 122, 126, 101]);
        let state = radio.read_back().expect("read back");
        assert_eq!(state, *radio.config());
        assert_eq!(radio.config().tuned_hz, DEFAULT_HZ);
        assert!(!radio.config().bias_tee, "the bias tee must start off");
    }

    #[test]
    fn a_dongle_in_its_bootloader_is_refused() {
        let link = FakeLink::default();
        sdrmm_device::lock(&link.0).bootloader = true;
        assert!(FunCube::open(link).is_err());
    }

    #[test]
    fn tuning_lands_where_the_firmware_says() {
        let (mut radio, link) = opened();
        radio.set_frequency_hz(145_800_000).expect("tune");
        assert_eq!(
            link.sent(),
            vec![(101, 145_800_000u32.to_le_bytes().to_vec())]
        );
        assert_eq!(radio.config().tuned_hz, 145_800_000);
        assert!((radio.config().center_hz() - 145_800_000.0).abs() < 1e-6);
    }

    #[test]
    fn a_frequency_outside_both_windows_never_reaches_the_dongle() {
        let (mut radio, link) = opened();
        assert!(radio.set_frequency_hz(300_000_000).is_err());
        assert!(radio.set_frequency_hz(100_000).is_err());
        assert!(link.sent().is_empty());
        assert_eq!(radio.config().asked_hz, DEFAULT_HZ);
    }

    #[test]
    fn a_fast_clock_is_tuned_low_and_reported_where_it_lands() {
        let (mut radio, link) = opened();
        radio.set_ppm(10.0).expect("ppm");
        let tuned = tuned_hz(DEFAULT_HZ, 10.0);
        assert_eq!(tuned, 99_999_000);
        assert_eq!(link.sent(), vec![(101, tuned.to_le_bytes().to_vec())]);
        assert!((radio.config().center_hz() - 99_999_999.99).abs() < 0.1);
    }

    #[test]
    fn ppm_stops_at_its_limit() {
        let (mut radio, link) = opened();
        assert!(radio.set_ppm(MAX_PPM + 1.0).is_err());
        assert!(radio.set_ppm(f64::NAN).is_err());
        assert!(link.sent().is_empty());
        assert!(validate_ppm(-MAX_PPM).is_ok());
    }

    #[test]
    fn retuning_to_the_same_frequency_sends_nothing() {
        let (mut radio, link) = opened();
        radio.set_frequency_hz(DEFAULT_HZ).expect("tune");
        assert!(link.sent().is_empty());
    }

    #[test]
    fn a_chosen_rf_filter_survives_retuning() {
        let (mut radio, link) = opened();
        radio.set_rf_filter(Some(RfFilter::Mhz145)).expect("filter");
        radio.set_frequency_hz(144_000_000).expect("tune");
        assert_eq!(
            link.sent(),
            vec![
                (113, vec![7, 0, 0, 0]),
                (101, 144_000_000u32.to_le_bytes().to_vec()),
                (113, vec![7, 0, 0, 0]),
            ]
        );
        assert_eq!(radio.active_rf_filter().expect("read"), RfFilter::Mhz145);
    }

    #[test]
    fn going_back_to_auto_lets_the_firmware_choose_again() {
        let (mut radio, _) = opened();
        radio
            .set_rf_filter(Some(RfFilter::Mhz0To4))
            .expect("filter");
        radio.set_rf_filter(None).expect("auto");
        assert_eq!(radio.config().rf_filter, None);
        assert_eq!(
            radio.active_rf_filter().expect("read"),
            RfFilter::Mhz75To125
        );
    }

    #[test]
    fn switches_and_gain_travel_as_one_byte() {
        let (mut radio, link) = opened();
        radio.set_lna(false).expect("lna");
        radio.set_mixer(false).expect("mixer");
        radio.set_if_gain_db(MAX_IF_GAIN_DB).expect("if");
        radio.set_if_filter(IfFilter::Khz1536).expect("filter");
        radio.set_bias_tee(true).expect("bias");
        assert_eq!(
            link.sent(),
            vec![
                (110, vec![0, 0, 0, 0]),
                (114, vec![0, 0, 0, 0]),
                (117, vec![59, 0, 0, 0]),
                (122, vec![3, 0, 0, 0]),
                (126, vec![1, 0, 0, 0]),
            ]
        );
        assert_eq!(radio.read_back().expect("read back"), *radio.config());
    }

    #[test]
    fn if_gain_beyond_the_stage_is_refused() {
        let (mut radio, link) = opened();
        assert!(radio.set_if_gain_db(MAX_IF_GAIN_DB + 1).is_err());
        assert!(link.sent().is_empty());
    }

    #[test]
    fn a_refused_write_leaves_the_state_as_it_was() {
        let (mut radio, link) = opened();
        sdrmm_device::lock(&link.0).refuse = Some(Command::SetBiasTee);
        assert!(matches!(
            radio.set_bias_tee(true),
            Err(Error::Rejected(Command::SetBiasTee))
        ));
        assert!(!radio.config().bias_tee);
        sdrmm_device::lock(&link.0).refuse = Some(Command::SetRfFilter);
        assert!(radio.set_rf_filter(Some(RfFilter::Mhz435)).is_err());
        assert_eq!(radio.config().rf_filter, None);
    }
}
