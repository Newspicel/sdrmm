pub struct Delay {
    buf: Vec<f32>,
    pos: usize,
}

impl Delay {
    #[must_use]
    pub fn new(samples: usize) -> Self {
        Self {
            buf: vec![0.0; samples],
            pos: 0,
        }
    }

    pub fn process(&mut self, x: &[f32], out: &mut Vec<f32>) {
        out.clear();
        if self.buf.is_empty() {
            out.extend_from_slice(x);
            return;
        }
        for &v in x {
            out.push(self.buf[self.pos]);
            self.buf[self.pos] = v;
            self.pos = (self.pos + 1) % self.buf.len();
        }
    }

    pub fn reset(&mut self) {
        self.buf.fill(0.0);
        self.pos = 0;
    }
}
