use super::{Aes, cmac, ctr, hex, hmac_sha256, increment_counter, sha256};

fn bytes(text: &str) -> Vec<u8> {
    hex(text).unwrap()
}

fn rfc4493_aes() -> Aes {
    Aes::new(&bytes("2b7e151628aed2a6abf7158809cf4f3c")).unwrap()
}

const RFC4493_MESSAGE: &str = "6bc1bee22e409f96e93d7e117393172a ae2d8a571e03ac9c9eb76fac45af8e51 \
                               30c81c46a35ce411e5fbc1191a0a52ef f69f2445df4f9b17ad2b417be66c3710";

fn rfc4493_case(length: usize, expected: &str) {
    let message = bytes(RFC4493_MESSAGE);
    assert_eq!(
        cmac(&rfc4493_aes(), &message[..length]).to_vec(),
        bytes(expected)
    );
}

#[test]
fn cmac_rfc4493_example_1_empty() {
    rfc4493_case(0, "bb1d6929e95937287fa37d129b756746");
}

#[test]
fn cmac_rfc4493_example_2_one_block() {
    rfc4493_case(16, "070a16b46b4d4144f79bdd9dd04a287c");
}

#[test]
fn cmac_rfc4493_example_3_partial_block() {
    rfc4493_case(40, "dfa66747de9ae63030ca32611497c827");
}

#[test]
fn cmac_rfc4493_example_4_four_blocks() {
    rfc4493_case(64, "51f0bebf7e3b9d92fc49741779363cfe");
}

#[test]
fn sha256_abc() {
    assert_eq!(
        sha256(&[b"a", b"bc"]).to_vec(),
        bytes("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
    );
}

#[test]
fn hmac_sha256_rfc4231_case_1() {
    assert_eq!(
        hmac_sha256(&[0x0b; 20], b"Hi There").to_vec(),
        bytes("b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7")
    );
}

#[test]
fn hmac_sha256_rfc4231_case_2() {
    assert_eq!(
        hmac_sha256(b"Jefe", b"what do ya want for nothing?").to_vec(),
        bytes("5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843")
    );
}

#[test]
fn hmac_sha256_rfc4231_case_3() {
    assert_eq!(
        hmac_sha256(&[0xaa; 20], &[0xdd; 50]).to_vec(),
        bytes("773ea91e36800e46854db8ebd09181a72959098b3ef8c122d9635514ced565fe")
    );
}

#[test]
fn hmac_sha256_rfc4231_case_6_long_key() {
    assert_eq!(
        hmac_sha256(
            &[0xaa; 131],
            b"Test Using Larger Than Block-Size Key - Hash Key First"
        )
        .to_vec(),
        bytes("60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54")
    );
}

#[test]
fn ctr_nist_sp800_38a_f51() {
    let aes = rfc4493_aes();
    let nonce: [u8; 16] = bytes("f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff")
        .try_into()
        .unwrap();
    let mut data = bytes(RFC4493_MESSAGE);
    ctr(&aes, &nonce, &mut data);
    assert_eq!(
        data,
        bytes(
            "874d6191b620e3261bef6864990db6ce 9806f66b7970fdff8617187bb9fffdff \
             5ae4df3edbd5d35e5b4f09020db03eab 1e031dda2fbe03d1792170a0f3009cee"
        )
    );
    ctr(&aes, &nonce, &mut data);
    assert_eq!(data, bytes(RFC4493_MESSAGE));
}

#[test]
fn ctr_counter_wraps_low_word_only() {
    let mut counter = [0x11u8; 16];
    counter[12..].copy_from_slice(&[0xff; 4]);
    increment_counter(&mut counter);
    assert_eq!(counter[..12], [0x11; 12]);
    assert_eq!(counter[12..], [0; 4]);
    increment_counter(&mut counter);
    assert_eq!(counter[12..], [0, 0, 0, 1]);
}

#[test]
fn ctr_keystream_matches_wrapped_counter() {
    let aes = rfc4493_aes();
    let mut nonce = [0x11u8; 16];
    nonce[12..].copy_from_slice(&[0xff; 4]);
    let plaintext: Vec<u8> = (0..37).collect();
    let mut data = plaintext.clone();
    ctr(&aes, &nonce, &mut data);
    let mut wrapped = nonce;
    wrapped[12..].copy_from_slice(&[0; 4]);
    aes.encrypt(&mut wrapped);
    let second_block: Vec<u8> = (16u8..32)
        .zip(wrapped)
        .map(|(byte, key)| byte ^ key)
        .collect();
    assert_eq!(data[16..32], second_block[..]);
    ctr(&aes, &nonce, &mut data);
    assert_eq!(data, plaintext);
}
