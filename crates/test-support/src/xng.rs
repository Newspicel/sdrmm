//! Lines our decoded JSON up with xng's for an equivalence check.

use serde_json::Value;

/// The ACARS fields xng has no equivalent of, so an equivalence check compares what xng claims
/// to decode and leaves our own decodes to the tests that cover them.
#[must_use]
pub fn without_fields_xng_drops(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        for field in ["oooi", "position", "met"] {
            object.remove(field);
        }
    }
    drop_hex_damaged(&mut value);
    value
}

/// ARINC 622 notes damaged hex that xng reports in its own shape, so it never takes part in an
/// equivalence check.
pub fn drop_hex_damaged(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("hex_damaged");
            for nested in object.values_mut() {
                drop_hex_damaged(nested);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(drop_hex_damaged),
        _ => {}
    }
}
