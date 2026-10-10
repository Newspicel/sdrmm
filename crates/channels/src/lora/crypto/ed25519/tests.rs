use super::{keypair, sign, verify};
use crate::lora::crypto::hex;

struct Vector {
    seed: &'static str,
    public_key: &'static str,
    message: &'static str,
    signature: &'static str,
}

const RFC8032_TEST_1: Vector = Vector {
    seed: "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
    public_key: "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
    message: "",
    signature: "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e06522490155\
                5fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
};

const RFC8032_TEST_2: Vector = Vector {
    seed: "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb",
    public_key: "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
    message: "72",
    signature: "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da\
                085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
};

const RFC8032_TEST_3: Vector = Vector {
    seed: "c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7",
    public_key: "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025",
    message: "af82",
    signature: "6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac\
                18ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a",
};

struct Decoded {
    seed: [u8; 32],
    public_key: [u8; 32],
    message: Vec<u8>,
    signature: [u8; 64],
}

fn decode(vector: &Vector) -> Decoded {
    Decoded {
        seed: hex(vector.seed).unwrap().try_into().unwrap(),
        public_key: hex(vector.public_key).unwrap().try_into().unwrap(),
        message: hex(vector.message).unwrap(),
        signature: hex(vector.signature).unwrap().try_into().unwrap(),
    }
}

fn check_vector(vector: &Vector) {
    let decoded = decode(vector);
    assert_eq!(keypair(&decoded.seed), decoded.public_key);
    assert_eq!(sign(&decoded.seed, &decoded.message), decoded.signature);
    assert!(verify(
        &decoded.public_key,
        &decoded.message,
        &decoded.signature
    ));
}

#[test]
fn rfc8032_test_1() {
    check_vector(&RFC8032_TEST_1);
}

#[test]
fn rfc8032_test_2() {
    check_vector(&RFC8032_TEST_2);
}

#[test]
fn rfc8032_test_3() {
    check_vector(&RFC8032_TEST_3);
}

#[test]
fn rejects_flipped_message_bit() {
    let decoded = decode(&RFC8032_TEST_3);
    let mut message = decoded.message.clone();
    message[1] ^= 0x01;
    assert!(!verify(&decoded.public_key, &message, &decoded.signature));
}

#[test]
fn rejects_flipped_signature_bits() {
    let decoded = decode(&RFC8032_TEST_2);
    for index in [0, 17, 32, 63] {
        let mut signature = decoded.signature;
        signature[index] ^= 0x04;
        assert!(!verify(&decoded.public_key, &decoded.message, &signature));
    }
}

#[test]
fn rejects_wrong_public_key() {
    let first = decode(&RFC8032_TEST_1);
    let second = decode(&RFC8032_TEST_2);
    assert!(!verify(
        &second.public_key,
        &first.message,
        &first.signature
    ));
}

#[test]
fn rejects_scalar_at_or_above_group_order() {
    let decoded = decode(&RFC8032_TEST_1);
    let order = hex("edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010").unwrap();
    let mut signature = decoded.signature;
    let mut carry = 0u16;
    for (byte, order_byte) in signature[32..].iter_mut().zip(&order) {
        let sum = u16::from(*byte) + u16::from(*order_byte) + carry;
        *byte = sum as u8;
        carry = sum >> 8;
    }
    assert_eq!(carry, 0);
    assert!(!verify(&decoded.public_key, &decoded.message, &signature));
    let mut at_order = decoded.signature;
    at_order[32..].copy_from_slice(&order);
    assert!(!verify(&decoded.public_key, &decoded.message, &at_order));
}

#[test]
fn rejects_invalid_point_encodings() {
    let decoded = decode(&RFC8032_TEST_1);
    let mut non_canonical_y = [0xffu8; 32];
    non_canonical_y[0] = 0xee;
    non_canonical_y[31] = 0x7f;
    assert!(!verify(
        &non_canonical_y,
        &decoded.message,
        &decoded.signature
    ));
    let mut off_curve = [0u8; 32];
    off_curve[0] = 2;
    assert!(!verify(&off_curve, &decoded.message, &decoded.signature));
    let mut negative_zero_x = [0u8; 32];
    negative_zero_x[0] = 1;
    negative_zero_x[31] = 0x80;
    assert!(!verify(
        &negative_zero_x,
        &decoded.message,
        &decoded.signature
    ));
    let mut bad_r = decoded.signature;
    bad_r[..32].copy_from_slice(&off_curve);
    assert!(!verify(&decoded.public_key, &decoded.message, &bad_r));
}

#[test]
fn signs_and_verifies_longer_message() {
    let seed = [0x42u8; 32];
    let message: Vec<u8> = (0..=255).collect();
    let public_key = keypair(&seed);
    let signature = sign(&seed, &message);
    assert!(verify(&public_key, &message, &signature));
    assert!(!verify(&public_key, &message[1..], &signature));
}
