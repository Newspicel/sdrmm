use crate::builtin::fixtures::{Positive, reads};

pub(crate) fn bss_decoder() -> Positive {
    reads(&[
        r#""source":"N0CALL""#,
        r#""message":"hello world""#,
        r#""altitude_m":100"#,
    ])
    .at(40.0, -75.0)
    .hearing(|| super::tests::fixture().heard())
}
