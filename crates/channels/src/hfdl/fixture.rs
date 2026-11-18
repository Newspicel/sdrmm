use crate::builtin::fixtures::{Positive, reads};

pub(crate) fn hfdl_decoder() -> Positive {
    reads(&[r#""gs_name":"Shannon, Ireland""#, r#""frame_index":1234"#])
        .hearing(|| super::tests::fixture().heard())
}
