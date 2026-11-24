use sdrmm_dsp::{CyclicCode, ParityCode, Soft, TrellisCode, golay23_decode_soft, soft};
use sdrmm_test_support::assert_no_alloc;

const HALF_RATE: TrellisCode<4, 4, 2> = TrellisCode::input_state([
    [[0, 2], [3, 0], [0, 1], [3, 3]],
    [[3, 2], [0, 0], [3, 1], [0, 3]],
    [[2, 1], [1, 3], [2, 2], [1, 0]],
    [[1, 1], [2, 3], [1, 2], [2, 0]],
]);

fn soften(word: u64, n: u32) -> Vec<Soft> {
    (0..n).map(|i| soft(word >> (n - 1 - i) & 1 == 1)).collect()
}

#[test]
fn soft_block_and_trellis_decoding_does_not_allocate() {
    let bch = CyclicCode::BCH_63_16;
    let nid = soften(bch.encode(0x293A) ^ 0x8001_0000_0400_0021, bch.n());
    let golay = [soft(true); 23];
    let hamming = [soft(false); 17];
    let mut word = [false; 17];
    let trellis = [[[soft(true), soft(false)]; 2]; 49];
    let mut inputs = [0u8; 49];
    let mut decoded = (None, (0, 0), None, 0);
    assert_no_alloc("soft fec", || {
        decoded = (
            bch.decode_soft(&nid),
            golay23_decode_soft(&golay),
            ParityCode::HAMMING_17_12.decode_soft(&hamming, &mut word),
            HALF_RATE.decode_soft(&trellis, 0, &mut inputs),
        );
    });
    assert_eq!(decoded.0, Some((0x293A, 5)));
    assert_eq!(decoded.1, (0x7F_FFFF, 0));
    assert_eq!(decoded.2, Some(0));
}
