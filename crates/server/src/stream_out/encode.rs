use std::{
    sync::{Arc, mpsc},
    time::Instant,
};

use sdrmm_decoder::VideoPicture;
use sdrmm_engine::{PcmBlock, PcmPayload};
use sdrmm_media::{CLOCK_HZ, TsEncoder};
use sdrmm_modem::dvb::PACKET;
use sdrmm_wire::StreamOutNode;

use super::{send::Outlet, stream::Stream};

pub(super) enum Input {
    Picture(Arc<VideoPicture>),
    Sound(PcmBlock),
}

/// Encodes what the wires bring until they stop, delivering every packet as it is made.
pub(super) fn run(
    queue: &mpsc::Receiver<Input>,
    stream: &Stream,
    settings: &StreamOutNode,
    with_audio: bool,
) {
    let mut outlet = match Outlet::open(settings, stream.viewers().clone()) {
        Ok(outlet) => outlet,
        Err(error) => {
            stream.fail(error);
            return;
        }
    };
    let mut encoder = TsEncoder::new(with_audio);
    let mut packets = Vec::new();
    let mut stereo = Vec::new();
    let start = Instant::now();
    while let Ok(item) = queue.recv() {
        if !outlet.wanted() {
            continue;
        }
        let now = (start.elapsed().as_secs_f64() * CLOCK_HZ as f64) as u64;
        let encoded = match item {
            Input::Picture(picture) => encoder
                .picture(&picture, now, &mut packets)
                .map(|()| stream.pictured(picture.width, picture.height)),
            Input::Sound(block) => encoder.audio(as_stereo(&block, &mut stereo), now, &mut packets),
        };
        let delivered = encoded.and_then(|()| outlet.deliver(&packets, now));
        packets.clear();
        match delivered {
            Ok(bytes) => {
                stream.sent(bytes);
                stream.recovered();
            }
            Err(error) => stream.fail(error),
        }
    }
}

/// A block of channel audio as interleaved stereo.
fn as_stereo<'a>(block: &PcmBlock, stereo: &'a mut Vec<f32>) -> &'a [f32] {
    stereo.clear();
    match &block.payload {
        PcmPayload::Silence(frames) => stereo.resize(frames * 2, 0.0),
        PcmPayload::Samples(samples) => {
            let lanes = usize::from(block.channels).max(1);
            for frame in samples.chunks_exact(lanes) {
                stereo.extend([frame[0], frame[usize::from(lanes > 1)]]);
            }
        }
    }
    stereo
}

/// Packets as the bytes a viewer reads.
pub(super) fn bytes_of(packets: &[[u8; PACKET]]) -> Vec<u8> {
    packets.concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_is_heard_on_both_sides_and_silence_keeps_its_length() {
        let mut stereo = Vec::new();
        let mono = PcmBlock {
            start_frame: 0,
            channels: 1,
            payload: PcmPayload::Samples(Arc::from([0.5, -0.25].as_slice())),
        };
        assert_eq!(as_stereo(&mono, &mut stereo), [0.5, 0.5, -0.25, -0.25]);
        let silent = PcmBlock {
            start_frame: 0,
            channels: 2,
            payload: PcmPayload::Silence(2),
        };
        assert_eq!(as_stereo(&silent, &mut stereo), [0.0; 4]);
    }
}
