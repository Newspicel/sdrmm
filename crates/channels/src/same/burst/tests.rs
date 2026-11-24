use super::*;
use crate::synth;

const HEADER: &str = "ZCZC-WXR-TOR-048201+0030-1232200-KEC55/NW-";
const LATER: u64 = WINDOW_SAMPLES + 1;

fn framed(bits: &[bool]) -> Vec<String> {
    let mut framer = Framer::default();
    bits.iter()
        .filter_map(|&bit| framer.push(if bit { 1.0 } else { -1.0 }))
        .filter_map(|burst| burst.text)
        .collect()
}

fn soft(text: &str) -> Vec<f32> {
    text.bytes()
        .flat_map(|byte| (0..8).map(move |bit| if byte >> bit & 1 == 1 { 1.0 } else { -1.0 }))
        .collect()
}

fn heard(text: &str) -> Burst {
    Burst {
        soft: soft(text),
        text: Some(text.to_owned()),
    }
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
    bursts.received(0, heard(HEADER));
    assert!(bursts.take().is_empty());
    bursts.received(1_000, heard(HEADER));
    let settled = bursts.take();
    assert_eq!(settled.len(), 1);
    assert_eq!(settled[0].copies, 2);
    assert_eq!(bursts.rejected(), 0);
}

#[test]
fn a_third_copy_of_a_settled_header_is_not_logged_again() {
    let mut bursts = Bursts::default();
    bursts.received(0, heard(HEADER));
    bursts.received(1_000, heard(HEADER));
    assert_eq!(bursts.take().len(), 1);
    bursts.received(2_000, heard(HEADER));
    assert!(bursts.take().is_empty());
    bursts.expire(LATER);
    assert_eq!(bursts.rejected(), 0);
}

#[test]
fn three_copies_that_differ_in_one_byte_settle_by_their_sum() {
    let mut bursts = Bursts::default();
    let bent = |at: usize, to: char| {
        let mut text = HEADER.to_owned();
        text.replace_range(at..=at, &to.to_string());
        text
    };
    bursts.received(0, heard(&bent(5, 'X')));
    bursts.received(1_000, heard(HEADER));
    bursts.received(2_000, heard(&bent(9, 'Y')));
    let settled = bursts.take();
    assert_eq!(settled.len(), 1);
    assert_eq!(settled[0].raw, HEADER);
    assert_eq!(settled[0].copies, 3);
    assert_eq!(bursts.rejected(), 0);
}

#[test]
fn three_copies_that_sum_to_no_header_are_rejected() {
    let mut bursts = Bursts::default();
    for (at, byte) in [(0, '!'), (1_000, '"'), (2_000, '$')] {
        let mut text = HEADER.to_owned();
        text.replace_range(4..5, &byte.to_string());
        bursts.received(at, heard(&text));
    }
    assert!(bursts.take().is_empty());
    assert_eq!(bursts.rejected(), 1);
}

#[test]
fn a_lone_copy_is_rejected_once_its_window_closes() {
    let mut bursts = Bursts::default();
    bursts.received(0, heard(HEADER));
    bursts.expire(WINDOW_SAMPLES);
    assert_eq!(bursts.rejected(), 0);
    bursts.expire(LATER);
    assert_eq!(bursts.rejected(), 1);
    assert!(bursts.take().is_empty());
}

#[test]
fn copies_further_apart_than_the_window_never_settle() {
    let mut bursts = Bursts::default();
    bursts.received(0, heard(HEADER));
    bursts.received(LATER, heard(HEADER));
    assert!(bursts.take().is_empty());
    assert_eq!(bursts.rejected(), 1);
}

#[test]
fn a_confident_copy_outweighs_two_weak_damaged_ones() {
    let mut damaged = soft(HEADER);
    damaged[8 * 10 + 7] = 0.2;
    let weak = || Burst {
        soft: damaged.clone(),
        text: None,
    };
    let mut bursts = Bursts::default();
    bursts.received(0, weak());
    bursts.received(1_000, weak());
    bursts.received(2_000, heard(HEADER));
    let settled = bursts.take();
    assert_eq!(settled.len(), 1);
    assert_eq!(settled[0].raw, HEADER);
    assert_eq!(settled[0].copies, 3);
}

#[test]
fn a_damaged_burst_is_kept_whole_for_summing() {
    let mut bits = synth::same::bits(HEADER);
    let at = bits.len() - 8 * 5 + 7;
    bits[at] = !bits[at];
    let mut framer = Framer::default();
    let mut bursts = bits
        .iter()
        .filter_map(|&bit| framer.push(if bit { 1.0 } else { -1.0 }));
    assert!(bursts.next().is_none());
    let mut filler = std::iter::repeat_n(-1.0, MAX_BITS);
    let burst = filler.find_map(|value| framer.push(value));
    let burst = burst.expect("the header runs to its longest");
    assert_eq!(burst.text, None);
    assert_eq!(burst.soft.len(), MAX_BITS);
    assert_eq!(read(&burst.soft[..8 * HEADER.len()]), None);
    assert_eq!(&burst.soft[..8 * 4], &soft("ZCZC")[..]);
}

#[test]
fn summing_keeps_the_longest_copy() {
    let summed = combine([[1.0, -1.0].as_slice(), [1.0].as_slice()].into_iter());
    assert_eq!(summed, vec![2.0, -1.0]);
}
