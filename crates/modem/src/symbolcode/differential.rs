#[derive(Clone, Debug, Default)]
pub struct DifferentialEncoder {
    last: bool,
}

impl DifferentialEncoder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn encode(&mut self, bit: bool) -> bool {
        self.last ^= bit;
        self.last
    }

    pub fn encode_all(&mut self, bits: &[bool]) -> Vec<bool> {
        bits.iter().map(|&b| self.encode(b)).collect()
    }

    pub fn reset(&mut self) {
        self.last = false;
    }
}

#[derive(Clone, Debug, Default)]
pub struct DifferentialDecoder {
    last: bool,
}

impl DifferentialDecoder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn decode(&mut self, bit: bool) -> bool {
        let out = bit ^ self.last;
        self.last = bit;
        out
    }

    pub fn decode_all(&mut self, bits: &[bool]) -> Vec<bool> {
        bits.iter().map(|&b| self.decode(b)).collect()
    }

    pub fn reset(&mut self) {
        self.last = false;
    }
}

#[derive(Clone, Debug)]
pub struct DifferentialSymbolEncoder {
    m: u32,
    last: u32,
}

impl DifferentialSymbolEncoder {
    #[must_use]
    pub fn new(m: u32) -> Self {
        assert!(m >= 2, "differential alphabet needs at least two symbols");
        Self { m, last: 0 }
    }

    pub fn encode(&mut self, symbol: u32) -> u32 {
        debug_assert!(symbol < self.m, "symbol {symbol} outside 0..{}", self.m);
        self.last = ((u64::from(symbol) + u64::from(self.last)) % u64::from(self.m)) as u32;
        self.last
    }

    pub fn encode_all(&mut self, symbols: &[u32]) -> Vec<u32> {
        symbols.iter().map(|&s| self.encode(s)).collect()
    }

    pub fn reset(&mut self) {
        self.last = 0;
    }
}

#[derive(Clone, Debug)]
pub struct DifferentialSymbolDecoder {
    m: u32,
    last: u32,
}

impl DifferentialSymbolDecoder {
    #[must_use]
    pub fn new(m: u32) -> Self {
        assert!(m >= 2, "differential alphabet needs at least two symbols");
        Self { m, last: 0 }
    }

    pub fn decode(&mut self, symbol: u32) -> u32 {
        debug_assert!(symbol < self.m, "symbol {symbol} outside 0..{}", self.m);
        let out = ((u64::from(symbol) + u64::from(self.m) - u64::from(self.last % self.m))
            % u64::from(self.m)) as u32;
        self.last = symbol;
        out
    }

    pub fn decode_all(&mut self, symbols: &[u32]) -> Vec<u32> {
        symbols.iter().map(|&s| self.decode(s)).collect()
    }

    pub fn reset(&mut self) {
        self.last = 0;
    }
}
