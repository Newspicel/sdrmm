#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Station {
    pub call: &'static str,
    pub frequency_hz: f64,
}

const fn station(call: &'static str, frequency_hz: f64) -> Station {
    Station { call, frequency_hz }
}

pub(crate) const STATIONS: &[Station] = &[
    station("JXN", 16_400.0),
    station("HWU", 18_300.0),
    station("GQD", 19_580.0),
    station("NWC", 19_800.0),
    station("ICV", 20_270.0),
    station("NPM", 21_400.0),
    station("HWU", 21_750.0),
    station("GQZ", 22_100.0),
    station("JJI", 22_200.0),
    station("DHO38", 23_400.0),
    station("NAA", 24_000.0),
    station("NLK", 24_800.0),
    station("NML", 25_200.0),
    station("TBB", 26_700.0),
    station("NRK", 37_500.0),
    station("NAU", 40_750.0),
    station("NSY", 45_900.0),
];

pub(crate) fn within(low_hz: f64, high_hz: f64) -> impl Iterator<Item = Station> {
    STATIONS
        .iter()
        .copied()
        .filter(move |station| (low_hz..=high_hz).contains(&station.frequency_hz))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_is_sorted_and_spaced_wider_than_a_station() {
        for pair in STATIONS.windows(2) {
            assert!(
                pair[1].frequency_hz - pair[0].frequency_hz >= 2.0 * super::super::STATION_HALF_HZ,
                "{pair:?}"
            );
        }
    }

    #[test]
    fn a_window_picks_the_stations_inside_it() {
        let calls: Vec<_> = within(23_000.0, 25_000.0).map(|s| s.call).collect();
        assert_eq!(calls, ["DHO38", "NAA", "NLK"]);
    }
}
