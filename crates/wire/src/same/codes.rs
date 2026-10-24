use std::sync::LazyLock;

const COUNTIES: &str = include_str!("../../data/same-counties.txt");

const ORIGINATORS: &[(&str, &str)] = &[
    ("EAS", "EAS participant"),
    ("CIV", "Civil authorities"),
    ("WXR", "National Weather Service"),
    ("PEP", "United States Government"),
];

const EVENTS: &[(&str, &str)] = &[
    ("ADR", "Administrative Message"),
    ("AVA", "Avalanche Watch"),
    ("AVW", "Avalanche Warning"),
    ("BLU", "Blue Alert"),
    ("BZW", "Blizzard Warning"),
    ("CAE", "Child Abduction Emergency"),
    ("CDW", "Civil Danger Warning"),
    ("CEM", "Civil Emergency Message"),
    ("CFA", "Coastal Flood Watch"),
    ("CFW", "Coastal Flood Warning"),
    ("DMO", "Practice/Demo Warning"),
    ("DSW", "Dust Storm Warning"),
    ("EAN", "Emergency Action Notification"),
    ("EQW", "Earthquake Warning"),
    ("EVI", "Evacuation Immediate"),
    ("EWW", "Extreme Wind Warning"),
    ("FFA", "Flash Flood Watch"),
    ("FFS", "Flash Flood Statement"),
    ("FFW", "Flash Flood Warning"),
    ("FLA", "Flood Watch"),
    ("FLS", "Flood Statement"),
    ("FLW", "Flood Warning"),
    ("FRW", "Fire Warning"),
    ("HLS", "Hurricane Statement"),
    ("HMW", "Hazardous Materials Warning"),
    ("HUA", "Hurricane Watch"),
    ("HUW", "Hurricane Warning"),
    ("HWA", "High Wind Watch"),
    ("HWW", "High Wind Warning"),
    ("LAE", "Local Area Emergency"),
    ("LEW", "Law Enforcement Warning"),
    ("MEP", "Missing and Endangered Persons"),
    ("NIC", "National Information Center"),
    ("NMN", "Network Message Notification"),
    ("NPT", "National Periodic Test"),
    ("NUW", "Nuclear Power Plant Warning"),
    ("RHW", "Radiological Hazard Warning"),
    ("RMT", "Required Monthly Test"),
    ("RWT", "Required Weekly Test"),
    ("SMW", "Special Marine Warning"),
    ("SPS", "Special Weather Statement"),
    ("SPW", "Shelter in Place Warning"),
    ("SQW", "Snow Squall Warning"),
    ("SSA", "Storm Surge Watch"),
    ("SSW", "Storm Surge Warning"),
    ("SVA", "Severe Thunderstorm Watch"),
    ("SVR", "Severe Thunderstorm Warning"),
    ("SVS", "Severe Weather Statement"),
    ("TOA", "Tornado Watch"),
    ("TOE", "911 Telephone Outage Emergency"),
    ("TOR", "Tornado Warning"),
    ("TRA", "Tropical Storm Watch"),
    ("TRW", "Tropical Storm Warning"),
    ("TSA", "Tsunami Watch"),
    ("TSW", "Tsunami Warning"),
    ("VOW", "Volcano Warning"),
    ("WSA", "Winter Storm Watch"),
    ("WSW", "Winter Storm Warning"),
];

const STATES: &[(u8, &str)] = &[
    (1, "AL"),
    (2, "AK"),
    (4, "AZ"),
    (5, "AR"),
    (6, "CA"),
    (8, "CO"),
    (9, "CT"),
    (10, "DE"),
    (11, "DC"),
    (12, "FL"),
    (13, "GA"),
    (15, "HI"),
    (16, "ID"),
    (17, "IL"),
    (18, "IN"),
    (19, "IA"),
    (20, "KS"),
    (21, "KY"),
    (22, "LA"),
    (23, "ME"),
    (24, "MD"),
    (25, "MA"),
    (26, "MI"),
    (27, "MN"),
    (28, "MS"),
    (29, "MO"),
    (30, "MT"),
    (31, "NE"),
    (32, "NV"),
    (33, "NH"),
    (34, "NJ"),
    (35, "NM"),
    (36, "NY"),
    (37, "NC"),
    (38, "ND"),
    (39, "OH"),
    (40, "OK"),
    (41, "OR"),
    (42, "PA"),
    (44, "RI"),
    (45, "SC"),
    (46, "SD"),
    (47, "TN"),
    (48, "TX"),
    (49, "UT"),
    (50, "VT"),
    (51, "VA"),
    (53, "WA"),
    (54, "WV"),
    (55, "WI"),
    (56, "WY"),
    (60, "AS"),
    (64, "FM"),
    (66, "GU"),
    (68, "MH"),
    (69, "MP"),
    (70, "PW"),
    (72, "PR"),
    (74, "UM"),
    (78, "VI"),
];

const PARTS: &[&str] = &[
    "north-west",
    "north",
    "north-east",
    "west",
    "central",
    "east",
    "south-west",
    "south",
    "south-east",
];

static COUNTY_INDEX: LazyLock<Vec<(u32, &'static str)>> = LazyLock::new(|| {
    COUNTIES
        .lines()
        .filter_map(|line| {
            let (code, name) = line.split_once(' ')?;
            Some((code.parse().ok()?, name))
        })
        .collect()
});

#[must_use]
pub fn originator(code: &str) -> Option<&'static str> {
    lookup(ORIGINATORS, code)
}

#[must_use]
pub fn event(code: &str) -> Option<&'static str> {
    lookup(EVENTS, code)
}

fn lookup(table: &[(&str, &'static str)], code: &str) -> Option<&'static str> {
    table
        .iter()
        .find(|(key, _)| *key == code)
        .map(|(_, name)| *name)
}

#[must_use]
pub fn state(fips: u8) -> Option<&'static str> {
    STATES
        .iter()
        .find(|(key, _)| *key == fips)
        .map(|(_, abbr)| *abbr)
}

#[must_use]
pub fn county(state_fips: u8, county_fips: u16) -> Option<&'static str> {
    let key = u32::from(state_fips) * 1_000 + u32::from(county_fips);
    COUNTY_INDEX
        .binary_search_by_key(&key, |(code, _)| *code)
        .ok()
        .map(|at| COUNTY_INDEX[at].1)
}

#[must_use]
pub fn part(digit: u8) -> Option<&'static str> {
    PARTS.get(usize::from(digit).checked_sub(1)?).copied()
}

#[must_use]
pub fn place(part_digit: u8, state_fips: u8, county_fips: u16) -> Option<String> {
    let abbr = state(state_fips)?;
    let area = match county_fips {
        0 => format!("all of {abbr}"),
        _ => format!("{}, {abbr}", county(state_fips, county_fips)?),
    };
    Some(match part(part_digit) {
        Some(part) => format!("{part} {area}"),
        None => area,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_county_in_the_published_list_is_indexed_and_sorted() {
        assert_eq!(COUNTY_INDEX.len(), COUNTIES.lines().count());
        assert!(COUNTY_INDEX.windows(2).all(|pair| pair[0].0 < pair[1].0));
    }

    #[test]
    fn a_county_resolves_through_its_state_and_county_fips() {
        assert_eq!(county(48, 201), Some("Harris"));
        assert_eq!(county(1, 1), Some("Autauga"));
        assert_eq!(county(78, 10), Some("St. Croix"));
        assert_eq!(county(48, 999), None);
    }

    #[test]
    fn a_zero_county_covers_the_whole_state() {
        assert_eq!(place(0, 48, 0).as_deref(), Some("all of TX"));
        assert_eq!(place(0, 48, 201).as_deref(), Some("Harris, TX"));
    }

    #[test]
    fn a_part_digit_names_the_ninth_of_the_county_it_covers() {
        assert_eq!(place(7, 48, 201).as_deref(), Some("south-west Harris, TX"));
        assert_eq!(part(0), None);
        assert_eq!(part(5), Some("central"));
        assert_eq!(part(10), None);
    }

    #[test]
    fn an_unknown_state_has_no_place_name() {
        assert_eq!(place(0, 3, 1), None);
        assert_eq!(state(3), None);
    }

    #[test]
    fn the_code_tables_are_sorted_and_free_of_duplicates() {
        assert!(EVENTS.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert!(STATES.windows(2).all(|pair| pair[0].0 < pair[1].0));
    }

    #[test]
    fn the_published_originator_and_event_codes_resolve() {
        assert_eq!(originator("WXR"), Some("National Weather Service"));
        assert_eq!(originator("ZZZ"), None);
        assert_eq!(event("TOR"), Some("Tornado Warning"));
        assert_eq!(event("RWT"), Some("Required Weekly Test"));
        assert_eq!(event("ZZZ"), None);
    }
}
