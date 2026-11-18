use serde_json::json;

use super::*;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Meter {
    #[serde(default)]
    blocks: u32,
}

#[test]
fn params_keep_the_tagged_wire_shape() {
    let params = ProcessorParams::new("meter", &Meter { blocks: 8 }).expect("serializes");
    let json = serde_json::to_value(&params).expect("params serialize");
    assert_eq!(json, json!({"type": "meter", "settings": {"blocks": 8}}));
    let back: ProcessorParams = serde_json::from_value(json).expect("params parse");
    assert_eq!(back.parse::<Meter>(), Ok(Meter { blocks: 8 }));
}

#[test]
fn missing_settings_take_the_processor_defaults() {
    let params: ProcessorParams =
        serde_json::from_value(json!({"type": "meter"})).expect("bare params");
    assert_eq!(params.parse::<Meter>(), Ok(Meter::default()));
    let null = ProcessorParams {
        settings: serde_json::Value::Null,
        ..ProcessorParams::bare("meter")
    };
    assert_eq!(null.parse::<Meter>(), Ok(Meter::default()));
}

#[test]
fn a_node_without_a_type_has_a_problem() {
    assert_eq!(ProcessorParams::default().problem(), Some(NO_TYPE));
    assert!(ProcessorParams::bare("meter").valid());
}

#[test]
fn readings_keep_the_tagged_wire_shape() {
    let reading = ProcessorReading {
        type_id: "meter".into(),
        reading: json!({"blocks": 3}),
    };
    let json = serde_json::to_value(&reading).expect("reading serializes");
    assert_eq!(json, json!({"type": "meter", "reading": {"blocks": 3}}));
    assert_eq!(reading.parse::<Meter>(), Ok(Meter { blocks: 3 }));
}

#[test]
fn reserved_at_holds_every_timestamp() {
    assert!(reserved_at().capacity() >= MAX_AT_LEN);
}
