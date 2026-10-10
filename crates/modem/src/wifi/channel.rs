pub const CHANNEL_WIDTH_HZ: f64 = 20_000_000.0;
pub const OCCUPIED_HZ: f64 = 16_562_500.0;
const GRID_HZ: f64 = 5_000_000.0;
const ON_GRID_HZ: f64 = 2_500_000.0;
const CHANNEL_14_HZ: f64 = 2_484_000_000.0;
const GHZ_2_4: Plan = Plan {
    band: WifiBand::Ghz2_4,
    start_hz: 2_407_000_000.0,
    low_hz: 2_400_000_000.0,
    high_hz: 2_500_000_000.0,
};
const GHZ_5: Plan = Plan {
    band: WifiBand::Ghz5,
    start_hz: 5_000_000_000.0,
    low_hz: 5_000_000_000.0,
    high_hz: 5_925_000_000.0,
};
const GHZ_6: Plan = Plan {
    band: WifiBand::Ghz6,
    start_hz: 5_950_000_000.0,
    low_hz: 5_925_000_000.0,
    high_hz: 7_125_000_000.0,
};
const TWENTY_MHZ_5: [std::ops::RangeInclusive<u8>; 3] = [36..=64, 100..=144, 149..=177];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WifiBand {
    Ghz2_4,
    Ghz5,
    Ghz6,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Channel {
    pub band: WifiBand,
    pub number: u8,
    pub centre_hz: f64,
}

struct Plan {
    band: WifiBand,
    start_hz: f64,
    low_hz: f64,
    high_hz: f64,
}

impl Plan {
    fn at(&self, number: u8) -> Channel {
        Channel {
            band: self.band,
            number,
            centre_hz: self.start_hz + f64::from(number) * GRID_HZ,
        }
    }
}

#[must_use]
pub fn channel_at(frequency_hz: f64) -> Option<Channel> {
    if (frequency_hz - CHANNEL_14_HZ).abs() < ON_GRID_HZ {
        return Some(Channel {
            band: WifiBand::Ghz2_4,
            number: 14,
            centre_hz: CHANNEL_14_HZ,
        });
    }
    let plan = [GHZ_2_4, GHZ_5, GHZ_6]
        .into_iter()
        .find(|plan| (plan.low_hz..plan.high_hz).contains(&frequency_hz))?;
    let number = ((frequency_hz - plan.start_hz) / GRID_HZ).round();
    (1.0..=233.0)
        .contains(&number)
        .then(|| plan.at(number as u8))
}

pub fn twenty_mhz() -> impl Iterator<Item = Channel> {
    let low = (1..=13).map(|number| GHZ_2_4.at(number)).chain([Channel {
        band: WifiBand::Ghz2_4,
        number: 14,
        centre_hz: CHANNEL_14_HZ,
    }]);
    let mid = TWENTY_MHZ_5
        .into_iter()
        .flat_map(|range| range.step_by(4))
        .map(|number| GHZ_5.at(number));
    let high = (1..=233).step_by(4).map(|number| GHZ_6.at(number));
    low.chain(mid).chain(high)
}

pub fn twenty_mhz_within(low_hz: f64, high_hz: f64) -> impl Iterator<Item = Channel> {
    twenty_mhz().filter(move |channel| {
        channel.centre_hz - OCCUPIED_HZ / 2.0 >= low_hz
            && channel.centre_hz + OCCUPIED_HZ / 2.0 <= high_hz
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centres_map_to_their_numbers() {
        let numbers = |hz: f64| channel_at(hz).map(|channel| (channel.band, channel.number));
        assert_eq!(numbers(2_412e6), Some((WifiBand::Ghz2_4, 1)));
        assert_eq!(numbers(2_437e6), Some((WifiBand::Ghz2_4, 6)));
        assert_eq!(numbers(2_484e6), Some((WifiBand::Ghz2_4, 14)));
        assert_eq!(numbers(5_745e6), Some((WifiBand::Ghz5, 149)));
        assert_eq!(numbers(5_955e6), Some((WifiBand::Ghz6, 1)));
        assert_eq!(numbers(915e6), None);
    }

    #[test]
    fn a_wide_window_on_2_4_ghz_holds_all_thirteen() {
        let numbers: Vec<u8> = twenty_mhz_within(2_402e6, 2_482e6)
            .map(|channel| channel.number)
            .collect();
        assert_eq!(numbers, (1..=13).collect::<Vec<u8>>());
        let one: Vec<u8> = twenty_mhz_within(2_427e6, 2_447e6)
            .map(|channel| channel.number)
            .collect();
        assert_eq!(one, vec![6]);
    }

    #[test]
    fn five_ghz_lists_only_twenty_megahertz_channels() {
        let numbers: Vec<u8> = twenty_mhz_within(5_160e6, 5_340e6)
            .map(|channel| channel.number)
            .collect();
        assert_eq!(numbers, vec![36, 40, 44, 48, 52, 56, 60, 64]);
    }
}
