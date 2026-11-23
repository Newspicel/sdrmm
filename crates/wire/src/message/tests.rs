use super::*;

fn names(name: &str) -> Option<String> {
    match name {
        "headline" => Some("DMR call".to_owned()),
        "freq" => Some("451.125".to_owned()),
        _ => None,
    }
}

#[test]
fn placeholders_take_their_values() {
    assert_eq!(
        fill("**{headline}** on {freq} MHz", names),
        "**DMR call** on 451.125 MHz"
    );
}

#[test]
fn an_unknown_name_leaves_nothing() {
    assert_eq!(fill("[{nope}]", names), "[]");
}

#[test]
fn spaces_inside_braces_are_ignored() {
    assert_eq!(fill("{ freq }", names), "451.125");
}

#[test]
fn doubled_braces_write_one() {
    assert_eq!(fill("{{freq}} }}", names), "{freq} }");
}

#[test]
fn an_open_brace_without_a_close_stays() {
    assert_eq!(fill("a { b\n{freq}", names), "a { b\n451.125");
    assert_eq!(fill("{", names), "{");
    assert_eq!(fill("}", names), "}");
}

#[test]
fn a_layout_past_the_limit_is_refused() {
    assert!(valid_message(&"x".repeat(MAX_MESSAGE_LEN)));
    assert!(!valid_message(&"x".repeat(MAX_MESSAGE_LEN + 1)));
}
