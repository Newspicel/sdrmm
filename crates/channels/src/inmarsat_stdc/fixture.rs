use crate::builtin::fixtures::{Positive, reads};

pub(crate) fn inmarsat_stdc_decoder() -> Positive {
    reads(&[r#""region":"AOR-W""#, r#""frame_number":1000"#])
        .hearing(|| super::tests::fixture().heard())
}
