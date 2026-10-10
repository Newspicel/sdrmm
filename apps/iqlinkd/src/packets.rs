use sdrmm_iqlink::{Header, frames_per_datagram};

#[derive(Debug)]
pub struct Packetizer {
    frame_bytes: usize,
    payload_bytes: usize,
    index: u64,
    board_lost: u64,
}

impl Packetizer {
    pub const fn new(frame_bytes: usize) -> Self {
        Self {
            frame_bytes,
            payload_bytes: frames_per_datagram(frame_bytes) * frame_bytes,
            index: 0,
            board_lost: 0,
        }
    }

    pub const fn payload_bytes(&self) -> usize {
        self.payload_bytes
    }

    pub const fn skip(&mut self, frames: u64, board_lost: u64) {
        self.index += frames;
        self.board_lost = board_lost;
    }

    pub fn datagrams<'a>(
        &'a mut self,
        chunk: &'a [u8],
    ) -> impl Iterator<Item = (Header, &'a [u8])> + 'a {
        let frame_bytes = self.frame_bytes.max(1);
        chunk
            .chunks(self.payload_bytes.max(frame_bytes))
            .map(move |payload| {
                let header = Header {
                    frame_bytes: frame_bytes as u16,
                    index: self.index,
                    board_lost: self.board_lost,
                };
                self.index += (payload.len() / frame_bytes) as u64;
                (header, payload)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chunk_is_cut_into_full_datagrams_and_one_short_tail() {
        let mut packets = Packetizer::new(4);
        let chunk = vec![7u8; 1448 * 2 + 40];
        let sent: Vec<(Header, usize)> = packets
            .datagrams(&chunk)
            .map(|(header, payload)| (header, payload.len()))
            .collect();
        assert_eq!(sent.len(), 3);
        assert_eq!(sent[0].1, 1448);
        assert_eq!(sent[2].1, 40);
        assert_eq!(sent[1].0.index, 362);
        assert_eq!(sent[2].0.index, 724);
        assert_eq!(sent[0].0.frame_bytes, 4);
    }

    #[test]
    fn skipped_frames_move_the_index_and_carry_the_board_loss() {
        let mut packets = Packetizer::new(8);
        let chunk = [0u8; 16];
        assert_eq!(packets.datagrams(&chunk).count(), 1);
        packets.skip(1_000, 1_000);
        let (header, _) = packets.datagrams(&chunk).next().expect("datagram");
        assert_eq!(header.index, 1_002);
        assert_eq!(header.board_lost, 1_000);
    }
}
