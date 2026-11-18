use crate::builtin::fixtures::{Positive, reads};

pub(crate) fn wmbus_decoder() -> Positive {
    reads(&[
        r#""manufacturer":"SEN""#,
        r#""id":"33225544""#,
        r#""device_type":7"#,
        r#""value":123.529"#,
    ])
    .hearing(|| super::tests::fixture().heard())
}
