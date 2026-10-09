const COMPANIES: &[(u16, &str)] = &[
    (0x0001, "Nokia"),
    (0x0002, "Intel"),
    (0x0003, "IBM"),
    (0x0006, "Microsoft"),
    (0x0008, "Motorola"),
    (0x000A, "Qualcomm"),
    (0x000D, "Texas Instruments"),
    (0x000F, "Broadcom"),
    (0x001D, "Qualcomm"),
    (0x0030, "STMicroelectronics"),
    (0x0046, "MediaTek"),
    (0x004C, "Apple"),
    (0x0057, "Harman"),
    (0x0059, "Nordic Semiconductor"),
    (0x0075, "Samsung"),
    (0x0078, "Nike"),
    (0x0087, "Garmin"),
    (0x009E, "Bose"),
    (0x00C4, "LG Electronics"),
    (0x00E0, "Google"),
    (0x012D, "Sony"),
    (0x0131, "Cypress Semiconductor"),
    (0x0157, "Huami"),
    (0x0171, "Amazon"),
    (0x018E, "Google"),
    (0x01DA, "Logitech"),
    (0x027D, "Huawei"),
    (0x02E5, "Espressif"),
    (0x02FF, "Silicon Labs"),
    (0x038F, "Xiaomi"),
    (0x0499, "Ruuvi"),
    (0x05A7, "Sonos"),
    (0x067C, "Tile"),
    (0x0822, "Adafruit"),
];

const SERVICES: &[(u16, &str)] = &[
    (0x1800, "Generic Access"),
    (0x1801, "Generic Attribute"),
    (0x1802, "Immediate Alert"),
    (0x1803, "Link Loss"),
    (0x1804, "Tx Power"),
    (0x1805, "Current Time"),
    (0x180A, "Device Information"),
    (0x180D, "Heart Rate"),
    (0x180F, "Battery"),
    (0x1810, "Blood Pressure"),
    (0x1812, "HID"),
    (0x1816, "Cycling Speed and Cadence"),
    (0x1818, "Cycling Power"),
    (0x1819, "Location and Navigation"),
    (0x181A, "Environmental Sensing"),
    (0x181B, "Body Composition"),
    (0x181D, "Weight Scale"),
    (0x1826, "Fitness Machine"),
    (0x1852, "Broadcast Audio"),
    (0x1856, "Public Broadcast"),
    (0xFCD2, "BTHome"),
    (0xFD5A, "Samsung SmartTag"),
    (0xFD6F, "Exposure Notification"),
    (0xFE03, "Amazon"),
    (0xFE2C, "Google Fast Pair"),
    (0xFE95, "Xiaomi"),
    (0xFE9F, "Google"),
    (0xFEAA, "Eddystone"),
    (0xFEEC, "Tile"),
    (0xFEED, "Tile"),
    (0xFFFA, "Remote ID"),
];

fn find(table: &'static [(u16, &'static str)], id: u16) -> Option<&'static str> {
    table
        .binary_search_by_key(&id, |&(key, _)| key)
        .ok()
        .map(|index| table[index].1)
}

#[must_use]
pub(crate) fn company(id: u16) -> Option<&'static str> {
    find(COMPANIES, id)
}

#[must_use]
pub(crate) fn service(id: u16) -> Option<&'static str> {
    find(SERVICES, id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_sorted_for_lookup() {
        assert!(COMPANIES.is_sorted_by_key(|&(id, _)| id));
        assert!(SERVICES.is_sorted_by_key(|&(id, _)| id));
        assert_eq!(company(0x004C), Some("Apple"));
        assert_eq!(service(0xFEAA), Some("Eddystone"));
        assert_eq!(company(0xFFFF), None);
    }
}
