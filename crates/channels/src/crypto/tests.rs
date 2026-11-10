use super::{
    Aes, Des, Rc4, aes_iv, cbc_decrypt, cmac, ctr, hex, hmac_sha256, increment_counter,
    next_message_indicator, ofb, sha256,
};

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

#[test]
fn cbc_nist_sp800_38a_f22() {
    let mut data = bytes(
        "7649abac8119b246cee98e9b12e9197d 5086cb9b507219ee95db113a917678b2 \
         73bed6b8e3c1743b7116e69e22229516 3ff1caa1681fac09120eca307586e1a7",
    );
    let iv: [u8; 16] = bytes("000102030405060708090a0b0c0d0e0f")
        .try_into()
        .unwrap();
    cbc_decrypt(&rfc4493_aes(), &iv, &mut data);
    assert_eq!(data, bytes(RFC4493_MESSAGE));
}

fn rc4_stream(key: &str, offset: usize) -> Vec<u8> {
    let mut cipher = Rc4::new(&bytes(key));
    cipher.skip(offset);
    let mut block = vec![0u8; 16];
    cipher.apply(&mut block);
    block
}

#[test]
fn rc4_rfc6229_forty_bit_key_at_offset_zero() {
    assert_eq!(
        rc4_stream("0102030405", 0),
        bytes("b2396305f03dc027ccc3524a0a1118a8")
    );
}

#[test]
fn rc4_rfc6229_forty_bit_key_after_dropping_256_bytes() {
    assert_eq!(
        rc4_stream("0102030405", 256),
        bytes("1cfcf62b03eddb641d77dfcf7f8d8c93")
    );
}

#[test]
fn rc4_rekeying_starts_the_keystream_over() {
    let mut cipher = Rc4::new(&bytes("0102030405"));
    cipher.skip(77);
    cipher.rekey(&bytes("0102030405"));
    let mut block = vec![0u8; 16];
    cipher.apply(&mut block);
    assert_eq!(block, bytes("b2396305f03dc027ccc3524a0a1118a8"));
}

#[test]
fn des_ofb_matches_openssl() {
    let des = Des::new([0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF]);
    let mut stream = [0u8; 24];
    ofb(
        |block| des.encrypt(block),
        [0x12, 0x34, 0x56, 0x78, 0x90, 0xAB, 0xCD, 0xEF],
        &mut stream,
    );
    assert_eq!(
        Some(stream.to_vec()),
        hex("bd661569ae874e255d976a504786581f5b0229c3443694e3")
    );
}

#[test]
fn aes_256_ofb_matches_openssl() {
    let key: Vec<u8> = (0..32).collect();
    let aes = Aes::new(&key).expect("aes-256 key");
    let iv: [u8; 16] = std::array::from_fn(|i| (i as u8) * 0x11);
    let mut stream = [0u8; 32];
    ofb(|block| aes.encrypt(block), iv, &mut stream);
    assert_eq!(
        Some(stream.to_vec()),
        hex("8ea2b7ca516745bfeafc49904b496089664a3455d8e9dbdb03158b52b93c288a")
    );
}

#[test]
fn the_message_indicator_lfsr_steps_64_bits() {
    assert_eq!(
        next_message_indicator(0x0123_4567_89AB_CDEF),
        0x20B1_25E7_79D0_F34E
    );
    assert_eq!(
        aes_iv(0x0123_4567_89AB_CDEF),
        0x0123_4567_89AB_CDEF_20B1_25E7_79D0_F34E_u128.to_be_bytes()
    );
}
