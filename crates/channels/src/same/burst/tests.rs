use super::*;
use crate::synth;

const HEADER: &str = "ZCZC-WXR-TOR-048201+0030-1232200-KEC55/NW-";
const LATER: u64 = WINDOW_SAMPLES + 1;

fn framed(bits: &[bool]) -> Vec<String> {
    let mut framer = Framer::default();
    bits.iter().filter_map(|&bit| framer.push(bit)).collect()
}

#[test]
fn the_preamble_sync_word_is_the_air_order_of_the_preamble_byte() {
    assert_eq!(preamble_word(1), 0xd5);
    assert_eq!(preamble_word(4), 0xd5d5_d5d5);
}

#[test]
fn a_preamble_and_header_frame_into_the_header_text() {
    assert_eq!(framed(&synth::same::bits(HEADER)), vec![HEADER.to_owned()]);
}

#[test]
fn back_to_back_bursts_each_frame() {
    let mut bits = synth::same::bits(HEADER);
    bits.extend(synth::same::bits("NNNN"));
    assert_eq!(framed(&bits), vec![HEADER.to_owned(), "NNNN".to_owned()]);
}

#[test]
fn a_preamble_with_nothing_after_it_frames_nothing() {
    assert!(framed(&synth::same::bits("")).is_empty());
}

#[test]
fn a_preamble_followed_by_rubbish_frames_nothing() {
    assert!(framed(&synth::same::bits("ZZZZ-NOT-A-HEADER")).is_empty());
}

#[test]
fn a_header_longer_than_the_grammar_allows_is_abandoned() {
    let long = format!("ZCZC{}", "-".repeat(same::MAX_HEADER_LEN));
    assert!(framed(&synth::same::bits(&long)).is_empty());
}

#[test]
fn bits_without_a_preamble_frame_nothing() {
    let bits = HEADER
        .bytes()
        .flat_map(|byte| (0..8).map(move |bit| byte >> bit & 1 == 1))
        .collect::<Vec<_>>();
    assert!(framed(&bits).is_empty());
}

#[test]
fn two_matching_copies_settle_the_header() {
    let mut bursts = Bursts::default();
    bursts.received(0, HEADER.to_owned());
    assert!(bursts.take().is_empty());
    bursts.received(1_000, HEADER.to_owned());
    let settled = bursts.take();
    assert_eq!(settled.len(), 1);
    assert_eq!(settled[0].copies, 2);
    assert_eq!(bursts.rejected(), 0);
}

#[test]
fn a_third_copy_of_a_settled_header_is_not_logged_again() {
    let mut bursts = Bursts::default();
    bursts.received(0, HEADER.to_owned());
    bursts.received(1_000, HEADER.to_owned());
    assert_eq!(bursts.take().len(), 1);
    bursts.received(2_000, HEADER.to_owned());
    assert!(bursts.take().is_empty());
    bursts.expire(LATER);
    assert_eq!(bursts.rejected(), 0);
}

#[test]
fn three_copies_that_differ_in_one_byte_settle_by_majority() {
    let mut bursts = Bursts::default();
    let bent = |at: usize, to: char| {
        let mut text = HEADER.to_owned();
        text.replace_range(at..at + 1, &to.to_string());
        text
    };
    bursts.received(0, bent(5, 'X'));
    bursts.received(1_000, HEADER.to_owned());
    bursts.received(2_000, bent(9, 'Y'));
    let settled = bursts.take();
    assert_eq!(settled.len(), 1);
    assert_eq!(settled[0].raw, HEADER);
    assert_eq!(settled[0].copies, 3);
    assert_eq!(bursts.rejected(), 0);
}

#[test]
fn three_copies_that_never_agree_are_rejected() {
    let mut bursts = Bursts::default();
    for (at, byte) in [(0, 'X'), (1_000, 'Y'), (2_000, 'Z')] {
        let mut text = HEADER.to_owned();
        text.replace_range(5..6, &byte.to_string());
        bursts.received(at, text);
    }
    assert!(bursts.take().is_empty());
    assert_eq!(bursts.rejected(), 1);
}

#[test]
fn a_lone_copy_is_rejected_once_its_window_closes() {
    let mut bursts = Bursts::default();
    bursts.received(0, HEADER.to_owned());
    bursts.expire(WINDOW_SAMPLES);
    assert_eq!(bursts.rejected(), 0);
    bursts.expire(LATER);
    assert_eq!(bursts.rejected(), 1);
    assert!(bursts.take().is_empty());
}

#[test]
fn copies_further_apart_than_the_window_never_settle() {
    let mut bursts = Bursts::default();
    bursts.received(0, HEADER.to_owned());
    bursts.received(LATER, HEADER.to_owned());
    assert!(bursts.take().is_empty());
    assert_eq!(bursts.rejected(), 1);
}

#[test]
fn copies_of_different_lengths_cannot_be_voted_on() {
    assert_eq!(majority(["abc", "abcd", "abc"]), None);
    assert_eq!(majority(["abc", "abd", "abc"]).as_deref(), Some("abc"));
    assert_eq!(majority(["abc", "abd", "abe"]), None);
    assert_eq!(majority(std::iter::empty()), None);
}
