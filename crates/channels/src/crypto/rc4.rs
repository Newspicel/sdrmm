/// The RC4 keystream generator. Keying sets up the whole state in place, so a voice path can rekey
/// every superframe without allocating.
#[derive(Clone)]
pub(crate) struct Rc4 {
    state: [u8; 256],
    i: u8,
    j: u8,
}

impl Rc4 {
    pub(crate) fn new(key: &[u8]) -> Self {
        let mut cipher = Self {
            state: [0; 256],
            i: 0,
            j: 0,
        };
        cipher.rekey(key);
        cipher
    }

    pub(crate) fn rekey(&mut self, key: &[u8]) {
        for (slot, value) in self.state.iter_mut().zip(0u8..=255) {
            *slot = value;
        }
        let mut j = 0u8;
        for i in 0..256 {
            j = j
                .wrapping_add(self.state[i])
                .wrapping_add(key[i % key.len()]);
            self.state.swap(i, usize::from(j));
        }
        self.i = 0;
        self.j = 0;
    }

    pub(crate) fn next_byte(&mut self) -> u8 {
        self.i = self.i.wrapping_add(1);
        self.j = self.j.wrapping_add(self.state[usize::from(self.i)]);
        self.state.swap(usize::from(self.i), usize::from(self.j));
        let index = self.state[usize::from(self.i)].wrapping_add(self.state[usize::from(self.j)]);
        self.state[usize::from(index)]
    }

    pub(crate) fn skip(&mut self, count: usize) {
        for _ in 0..count {
            self.next_byte();
        }
    }

    pub(crate) fn apply(&mut self, data: &mut [u8]) {
        for byte in data {
            *byte ^= self.next_byte();
        }
    }
}
