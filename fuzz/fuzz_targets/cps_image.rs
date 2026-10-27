#![no_main]

use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use sdrmm_cps::{Image, image::changed_blocks, models};

static BLANKS: OnceLock<Vec<Image>> = OnceLock::new();

fn blanks() -> Vec<Image> {
    models().iter().map(|radio| radio.blank_image()).collect()
}

fuzz_target!(|data: &[u8]| {
    let Some((&block, bytes)) = data.split_first() else {
        return;
    };
    let Some(image) = Image::from_bytes(bytes) else {
        return;
    };
    assert_eq!(
        Image::from_bytes(&image.to_bytes()).as_ref(),
        Some(&image),
        "an image did not survive a round trip through its own bytes"
    );
    let block = u32::from(block).max(1);
    for (radio, blank) in models().iter().zip(BLANKS.get_or_init(blanks)) {
        let _ = changed_blocks(blank, &image, block);
        let Ok(codeplug) = radio.decode(&image) else {
            continue;
        };
        let mut written = blank.clone();
        let Ok(report) = radio.encode(&codeplug, &mut written) else {
            continue;
        };
        let _ = changed_blocks(&image, &written, block);
        let Ok(again) = radio.decode(&written) else {
            continue;
        };
        assert_eq!(
            report.target_model,
            radio.descriptor().id,
            "a codeplug was reported against the wrong radio"
        );
        assert_eq!(
            usize::try_from(report.after.channels).unwrap_or(usize::MAX),
            again.channels.len(),
            "{} reported channels the radio did not read back",
            radio.descriptor().id
        );
        assert!(
            report.after.channels == report.before.channels || report.dropped() > 0,
            "{} dropped channels without reporting one",
            radio.descriptor().id
        );
    }
});
