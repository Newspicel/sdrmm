use crate::builtin::fixtures::{Positive, reads};

pub(crate) fn vdl2_decoder() -> Positive {
    reads(&[
        r#""station":"VT-ANB""#,
        r#""flight":"AI0142""#,
        r#""label":"B6""#,
    ])
    .at(52.0402, 19.8039)
    .hearing(|| super::tests::fixture().heard())
}
