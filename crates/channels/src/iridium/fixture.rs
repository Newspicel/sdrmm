use crate::builtin::fixtures::{Positive, reads};

pub(crate) fn iridium_decoder() -> Positive {
    reads(&[r#""sat":44"#, r#""beam":25"#, r#""tmsi":"071ca54a""#])
        .at(40.1088, -127.2933)
        .hearing(|| super::tests::fixture().heard())
}
