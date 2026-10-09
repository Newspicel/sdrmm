use sdrmm_wire::IsmKind;

const MHZ: f64 = 1e6;
const NARROW_HZ: f64 = 3.0 * MHZ;
const WIFI_40: std::ops::RangeInclusive<f64> = 32.0 * MHZ..=44.0 * MHZ;
const WIFI_FAINT: std::ops::RangeInclusive<f64> = 4.0 * MHZ..=24.0 * MHZ;
const WIFI_LONGEST_S: f64 = 6e-3;
const WIFI_GRID_START_HZ: f64 = 2_412.0 * MHZ;
const WIFI_GRID_HZ: f64 = 5.0 * MHZ;
const WIFI_GRID_SLACK_HZ: f64 = 1.5 * MHZ;
const OVEN: std::ops::RangeInclusive<f64> = 6e-3..=14e-3;
const OVEN_BAND: std::ops::RangeInclusive<f64> = 2_420.0 * MHZ..=2_490.0 * MHZ;
const OVEN_WIDTH_HZ: f64 = 2.0 * MHZ;
const BLE_START_HZ: f64 = 2_402.0 * MHZ;
const BLE_GRID_HZ: f64 = 2.0 * MHZ;
const BLE_CHANNELS: f64 = 40.0;
const CLASSIC_GRID_HZ: f64 = MHZ;
const CLASSIC_CHANNELS: f64 = 79.0;
const BLUETOOTH_LONGEST_S: f64 = 3e-3;
const CODED_LONGEST_S: f64 = 18e-3;
const ZIGBEE_START_HZ: f64 = 2_405.0 * MHZ;
const ZIGBEE_GRID_HZ: f64 = 5.0 * MHZ;
const ZIGBEE_CHANNELS: f64 = 16.0;
const ZIGBEE_SHORTEST_S: f64 = 150e-6;
const ZIGBEE_WIDEST_FROM_HZ: f64 = 1.2 * MHZ;
const ON_GRID_HZ: f64 = 0.4 * MHZ;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Shape {
    pub centre_hz: f64,
    pub width_hz: f64,
    pub duration_s: f64,
    pub edge: bool,
    pub cut: bool,
}

fn on_grid(frequency_hz: f64, start_hz: f64, step_hz: f64, count: f64, slack_hz: f64) -> bool {
    let steps = ((frequency_hz - start_hz) / step_hz).round();
    (0.0..count).contains(&steps) && (frequency_hz - start_hz - steps * step_hz).abs() <= slack_hz
}

fn wifi(shape: &Shape) -> bool {
    if shape.duration_s > WIFI_LONGEST_S {
        return false;
    }
    let gridded = on_grid(
        shape.centre_hz,
        WIFI_GRID_START_HZ,
        WIFI_GRID_HZ,
        f64::INFINITY,
        WIFI_GRID_SLACK_HZ,
    );
    let faint = WIFI_FAINT.contains(&shape.width_hz);
    ((gridded || shape.edge) && faint) || WIFI_40.contains(&shape.width_hz)
}

fn oven(shape: &Shape) -> bool {
    OVEN.contains(&shape.duration_s)
        && shape.width_hz >= OVEN_WIDTH_HZ
        && OVEN_BAND.contains(&shape.centre_hz)
}

fn narrow(shape: &Shape) -> IsmKind {
    let ble = on_grid(
        shape.centre_hz,
        BLE_START_HZ,
        BLE_GRID_HZ,
        BLE_CHANNELS,
        ON_GRID_HZ,
    );
    let classic = on_grid(
        shape.centre_hz,
        BLE_START_HZ,
        CLASSIC_GRID_HZ,
        CLASSIC_CHANNELS,
        ON_GRID_HZ,
    );
    let zigbee = on_grid(
        shape.centre_hz,
        ZIGBEE_START_HZ,
        ZIGBEE_GRID_HZ,
        ZIGBEE_CHANNELS,
        ON_GRID_HZ,
    );
    let zigbee_shaped = shape.duration_s >= ZIGBEE_SHORTEST_S
        && shape.width_hz >= ZIGBEE_WIDEST_FROM_HZ
        && (!ble || shape.duration_s > BLUETOOTH_LONGEST_S);
    if zigbee && zigbee_shaped {
        IsmKind::Ieee802154
    } else if (classic && shape.duration_s <= BLUETOOTH_LONGEST_S)
        || (ble && shape.duration_s <= CODED_LONGEST_S)
    {
        IsmKind::Bluetooth
    } else {
        IsmKind::Narrowband
    }
}

#[must_use]
pub(crate) fn classify(shape: &Shape) -> IsmKind {
    if shape.cut {
        IsmKind::Continuous
    } else if wifi(shape) {
        IsmKind::Wifi
    } else if oven(shape) {
        IsmKind::MicrowaveOven
    } else if shape.width_hz <= NARROW_HZ {
        narrow(shape)
    } else {
        IsmKind::Wideband
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(centre_mhz: f64, width_mhz: f64, duration_us: f64) -> Shape {
        Shape {
            centre_hz: centre_mhz * MHZ,
            width_hz: width_mhz * MHZ,
            duration_s: duration_us * 1e-6,
            edge: false,
            cut: false,
        }
    }

    #[test]
    fn wifi_is_wide_and_on_its_channel_grid() {
        assert_eq!(classify(&shape(2_437.2, 16.9, 300.0)), IsmKind::Wifi);
        assert_eq!(classify(&shape(2_442.0, 40.0, 300.0)), IsmKind::Wifi);
        assert_eq!(classify(&shape(2_439.5, 16.9, 300.0)), IsmKind::Wideband);
        assert_eq!(classify(&shape(2_436.0, 5.0, 400.0)), IsmKind::Wifi);
        assert_eq!(classify(&shape(2_450.0, 8.0, 400.0)), IsmKind::Wideband);
        let cut = Shape {
            edge: true,
            ..shape(2_446.0, 7.0, 200.0)
        };
        assert_eq!(classify(&cut), IsmKind::Wifi);
    }

    #[test]
    fn narrow_bursts_split_by_grid_and_length() {
        assert_eq!(classify(&shape(2_426.1, 1.6, 376.0)), IsmKind::Bluetooth);
        assert_eq!(classify(&shape(2_441.0, 1.0, 366.0)), IsmKind::Bluetooth);
        assert_eq!(classify(&shape(2_425.0, 2.2, 1_200.0)), IsmKind::Ieee802154);
        assert_eq!(classify(&shape(2_420.0, 2.2, 1_200.0)), IsmKind::Bluetooth);
        assert_eq!(classify(&shape(2_420.0, 2.2, 4_000.0)), IsmKind::Ieee802154);
        assert_eq!(
            classify(&shape(2_430.5, 0.6, 50_000.0)),
            IsmKind::Narrowband
        );
    }

    #[test]
    fn long_and_broad_is_an_oven_and_a_cut_burst_is_continuous() {
        assert_eq!(
            classify(&shape(2_455.0, 8.0, 9_000.0)),
            IsmKind::MicrowaveOven
        );
        let carrier = Shape {
            cut: true,
            ..shape(2_440.0, 0.3, 100_000.0)
        };
        assert_eq!(classify(&carrier), IsmKind::Continuous);
        assert_eq!(classify(&shape(2_440.0, 5.0, 900.0)), IsmKind::Wideband);
    }
}
