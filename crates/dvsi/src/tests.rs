use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use sdrmm_channels::chip::{
    CHIP_FRAME_SAMPLES, ChipFrame, ChipHealth, ChipLink, ChipRate, ChipSpeech,
    MAX_CHIP_FRAME_BYTES, VocoderChips,
};

use crate::{BAUDS, Chip, ChipError, Chips, fake::FakeChip};

const PATIENCE: Duration = Duration::from_secs(3);

fn voice(first: u8, played: bool) -> ChipFrame {
    let mut bits = [0u8; MAX_CHIP_FRAME_BYTES];
    bits[0] = first;
    bits[1] = 0xA5;
    ChipFrame::Voice { bits, played }
}

fn speech_for(first: u8) -> ChipSpeech {
    std::array::from_fn(|index| FakeChip::sample(&[first], index))
}

fn wait_for<T>(mut ready: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + PATIENCE;
    loop {
        if let Some(value) = ready() {
            return value;
        }
        assert!(Instant::now() < deadline, "timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn wait_health(link: &ChipLink, health: ChipHealth) {
    wait_for(|| (link.health() == health).then_some(()));
}

fn hub(chip: &FakeChip) -> Chips {
    Chips::with_opener(Arc::new(chip.clone()))
}

#[test]
fn a_thumbdv_answers_at_460800_baud() {
    let fake = FakeChip::new("AMBE3000R", 460_800);
    let chip = Chip::probe(&fake, "/dev/ttyUSB0", &BAUDS).expect("probe");
    assert_eq!(chip.info().product, "AMBE3000R");
    assert_eq!((chip.info().baud, chip.info().channels), (460_800, 1));
    assert_eq!(chip.info().version, "V120.E100.XXXX");
}

#[test]
fn a_usb_3003_is_found_at_its_own_baud_with_three_channels() {
    let fake = FakeChip::new("AMBE3003", 921_600);
    let chip = Chip::probe(&fake, "/dev/ttyUSB0", &BAUDS).expect("probe");
    assert_eq!((chip.info().baud, chip.info().channels), (921_600, 3));
}

#[test]
fn a_port_without_a_chip_says_so() {
    let fake = FakeChip::new("AMBE3000R", 115_200);
    assert!(matches!(
        Chip::probe(&fake, "/dev/ttyUSB0", &BAUDS),
        Err(ChipError::Silent(_))
    ));
}

#[test]
fn a_frame_comes_back_as_the_chips_speech() {
    let fake = FakeChip::new("AMBE3000R", 460_800);
    let chips = hub(&fake);
    let mut link = chips.open("/dev/ttyUSB0", ChipRate::Half).expect("open");
    assert_eq!(link.name(), "AMBE3000R");
    assert!(link.send(voice(3, false)));
    assert!(link.send(ChipFrame::Silence));
    assert!(link.send(voice(4, false)));
    assert_eq!(wait_for(|| link.receive()), speech_for(3));
    assert_eq!(wait_for(|| link.receive()), [0; CHIP_FRAME_SAMPLES]);
    assert_eq!(wait_for(|| link.receive()), speech_for(4));
    let state = fake.state();
    assert_eq!(state.heard[0].bit_count, 49);
    assert_eq!(state.heard[0].bits, [3, 0xA5, 0, 0, 0, 0, 0]);
    assert_eq!(state.heard[0].channel, None);
    assert_eq!(state.configured[0].1[..2], [0x09, 34]);
}

#[test]
fn a_played_frame_keeps_the_chip_in_step_without_speech() {
    let fake = FakeChip::new("AMBE3000R", 460_800);
    let chips = hub(&fake);
    let mut link = chips.open("/dev/ttyUSB0", ChipRate::Full).expect("open");
    assert!(link.send(voice(1, true)));
    assert!(link.send(voice(2, false)));
    assert_eq!(wait_for(|| link.receive()), speech_for(2));
    assert_eq!(fake.state().heard.len(), 2);
    assert_eq!(fake.state().heard[0].bit_count, 88);
}

#[test]
fn a_single_channel_chip_is_busy_for_a_second_call_until_the_first_ends() {
    let fake = FakeChip::new("AMBE3000R", 460_800);
    let chips = hub(&fake);
    let mut first = chips.open("/dev/ttyUSB0", ChipRate::Half).expect("open");
    let mut second = chips.open("/dev/ttyUSB0", ChipRate::Half).expect("open");
    assert!(first.send(voice(1, false)));
    wait_for(|| first.receive());
    assert!(second.send(voice(2, false)));
    wait_health(&second, ChipHealth::Busy);
    assert_eq!(wait_for(|| second.receive()), [0; CHIP_FRAME_SAMPLES]);
    wait_for(|| {
        second.send(voice(2, true));
        (second.health() == ChipHealth::Ready).then_some(())
    });
    assert_eq!(first.health(), ChipHealth::Ready);
}

#[test]
fn a_usb_3003_decodes_three_calls_on_three_channels() {
    let fake = FakeChip::new("AMBE3003", 921_600);
    let chips = hub(&fake);
    let mut links: Vec<ChipLink> = [ChipRate::Half, ChipRate::Full, ChipRate::Dstar]
        .into_iter()
        .map(|rate| chips.open("/dev/ttyUSB1", rate).expect("open"))
        .collect();
    for (index, link) in links.iter_mut().enumerate() {
        assert!(link.send(voice(u8::try_from(index).expect("index") + 10, false)));
    }
    for (index, link) in links.iter_mut().enumerate() {
        let first = u8::try_from(index).expect("index") + 10;
        assert_eq!(wait_for(|| link.receive()), speech_for(first));
        assert_eq!(link.health(), ChipHealth::Ready);
    }
    let state = fake.state();
    let channels: Vec<Option<u8>> = state.heard.iter().map(|heard| heard.channel).collect();
    assert_eq!(channels, [Some(0), Some(1), Some(2)]);
    assert_eq!(state.heard[2].bit_count, 72);
}

#[test]
fn a_reset_restarts_the_leased_decoder() {
    let fake = FakeChip::new("AMBE3000R", 460_800);
    let chips = hub(&fake);
    let mut link = chips.open("/dev/ttyUSB0", ChipRate::Half).expect("open");
    assert!(link.send(voice(1, false)));
    wait_for(|| link.receive());
    assert!(link.send(ChipFrame::Reset));
    wait_for(|| (!fake.state().restarts.is_empty()).then_some(()));
}

#[test]
fn an_unplugged_chip_is_lost_and_found_again() {
    let fake = FakeChip::new("AMBE3000R", 460_800);
    let chips = hub(&fake);
    let mut link = chips.open("/dev/ttyUSB0", ChipRate::Half).expect("open");
    fake.state().unplugged = true;
    assert!(link.send(voice(1, false)));
    wait_health(&link, ChipHealth::Lost);
    fake.state().unplugged = false;
    wait_health(&link, ChipHealth::Ready);
    assert!(link.send(voice(5, false)));
    assert_eq!(wait_for(|| link.receive()), speech_for(5));
}

#[test]
fn opening_a_missing_port_is_an_error() {
    let fake = FakeChip::new("AMBE3000R", 460_800);
    fake.state().unplugged = true;
    let error = hub(&fake)
        .open("/dev/ttyUSB9", ChipRate::Half)
        .err()
        .expect("an error");
    assert!(error.contains("no such device"), "{error}");
}
