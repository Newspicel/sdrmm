use super::*;

const CTY: &str = "Bouvet:                   38:  67:  AF:  -54.42:    -3.38:    -1.0:  3Y/b:
    =3Y/LB5SH;
Fed. Rep. of Germany:     14:  28:  EU:   51.00:   -10.00:    -1.0:  DL:
    DA,DB,DC,DD,DE,DF,DG,DH,DI,DJ,DK,DL,DM,DN,DO,DP,DQ,DR,Y2,Y3;
Spain:                    14:  37:  EU:   40.32:     3.43:    -1.0:  EA:
    AM,AN,AO,EA,EB,EC,ED,EE,EF,EG,EH;
Canary Islands:           33:  36:  AF:   28.32:    15.85:     0.0:  EA8:
    AM8,AN8,AO8,EA8,EB8,EC8,ED8,EE8,EF8,EG8,EH8;
Italy:                    15:  28:  EU:   42.82:   -12.58:    -1.0:  I:
    I;
Sicily:                   15:  28:  EU:   37.50:   -14.00:    -1.0:  *IT9:
    IT9,IW9;
United States:            05:  08:  NA:   37.60:    91.87:     5.0:  K:
    AA,K,N,W,=W1AW(5)[8],=KL7XX(1)[2]{OC};
Hawaii:                   31:  61:  OC:   21.12:   157.48:    10.0:  KH6:
    AH6,KH6,KH7,NH6,WH6,=K2GT;
European Russia:          16:  29:  EU:   53.65:   -41.37:    -4.0:  UA:
    R,U;
Asiatic Russia:           17:  30:  AS:   55.88:   -84.08:    -7.0:  UA9:
    R9,UA9;
Canada:                   05:  09:  NA:   44.35:    78.75:     5.0:  VE:
    VE,VA,=VER20260915;
";

fn table() -> Prefixes {
    Prefixes::parse(CTY.as_bytes()).expect("parses")
}

fn country(call: &str) -> Option<String> {
    table().lookup(call).map(|found| found.country)
}

#[test]
fn a_callsign_resolves_to_its_country_and_zones() {
    let found = table().lookup("dl1abc").expect("found");
    assert_eq!(found.call, "DL1ABC");
    assert_eq!(found.country, "Fed. Rep. of Germany");
    assert_eq!(found.prefix, "DL");
    assert_eq!(found.continent, "EU");
    assert_eq!((found.cq_zone, found.itu_zone), (14, 28));
}

#[test]
fn the_longest_prefix_wins() {
    assert_eq!(country("EA8ABC").as_deref(), Some("Canary Islands"));
    assert_eq!(country("EA1ABC").as_deref(), Some("Spain"));
    assert_eq!(country("UA9XYZ").as_deref(), Some("Asiatic Russia"));
    assert_eq!(country("UA3XYZ").as_deref(), Some("European Russia"));
}

#[test]
fn exact_calls_beat_prefixes_and_carry_their_own_zones() {
    assert_eq!(country("K2GT").as_deref(), Some("Hawaii"));
    assert_eq!(country("3Y/LB5SH").as_deref(), Some("Bouvet"));
    let odd = table().lookup("KL7XX").expect("found");
    assert_eq!(
        (odd.cq_zone, odd.itu_zone, odd.continent.as_str()),
        (1, 2, "OC")
    );
}

#[test]
fn portable_calls_are_placed_where_they_operate() {
    assert_eq!(country("DL/W1ABC").as_deref(), Some("Fed. Rep. of Germany"));
    assert_eq!(country("W1ABC/KH6").as_deref(), Some("Hawaii"));
    assert_eq!(country("EA8/DL1ABC/P").as_deref(), Some("Canary Islands"));
    assert_eq!(country("W1ABC/4").as_deref(), Some("United States"));
    assert_eq!(country("DL1ABC/MM"), None);
}

#[test]
fn ssids_and_brackets_are_stripped() {
    assert_eq!(
        table().lookup("DL1ABC-9").map(|f| f.call).as_deref(),
        Some("DL1ABC")
    );
    assert_eq!(
        table().lookup("<W1AW>").map(|f| f.call).as_deref(),
        Some("W1AW")
    );
    assert_eq!(
        table().lookup("DL1ABC B").map(|f| f.call).as_deref(),
        Some("DL1ABC")
    );
}

#[test]
fn words_that_are_not_callsigns_stay_unresolved() {
    for word in ["DX", "CQ", "<...>", "145", "", "D!1ABC"] {
        assert_eq!(country(word), None, "{word}");
    }
}

#[test]
fn wae_only_entities_fall_back_to_their_dxcc_country() {
    assert_eq!(country("IT9ABC").as_deref(), Some("Italy"));
}

#[test]
fn the_version_is_read_and_a_broken_list_is_refused() {
    assert_eq!(table().version, "20260915");
    assert!(Prefixes::parse(CTY.replace("=VER20260915", "VA").as_bytes()).is_err());
    assert!(Prefixes::parse(CTY.replace("  14:  28:  EU", "  14:  28:  XX").as_bytes()).is_err());
    assert!(Prefixes::parse(b"<html>=VER20260915</html>").is_err());
    assert!(Prefixes::parse(&[0xff, 0xfe]).is_err());
}
