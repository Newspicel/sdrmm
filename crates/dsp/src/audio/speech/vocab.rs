const WORD_START: char = '\u{2581}';

/// Token pieces as bytes; byte-fallback pieces are their one byte and control tokens are empty.
pub(super) struct Vocab {
    pieces: Vec<Vec<u8>>,
}

impl Vocab {
    /// Pieces one per line, in token order.
    pub fn parse(lines: &str) -> Self {
        Self {
            pieces: lines.split('\n').map(piece_bytes).collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.pieces.len()
    }

    pub fn decode(&self, tokens: &[usize]) -> String {
        let bytes: Vec<u8> = tokens
            .iter()
            .filter_map(|&token| self.pieces.get(token))
            .flatten()
            .copied()
            .collect();
        String::from_utf8_lossy(&bytes)
            .replace(WORD_START, " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn piece_bytes(piece: &str) -> Vec<u8> {
    piece
        .strip_prefix("<0x")
        .and_then(|rest| rest.strip_suffix('>'))
        .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        .map_or_else(|| piece.as_bytes().to_vec(), |byte| vec![byte])
}
