use super::kernel::Sample;

const MIN_ROOM: usize = 2048;

#[derive(Clone, Debug)]
pub(crate) struct DelayLine<T> {
    samples: Vec<T>,
    phases: usize,
    stride: usize,
    capacity: usize,
    history: usize,
    start: usize,
    filled: usize,
}

impl<T: Sample> DelayLine<T> {
    pub(crate) fn new(history: usize, phases: usize, slack: usize) -> Self {
        let capacity = (history + MIN_ROOM.max(4 * (history + 1))).div_ceil(phases);
        let stride = capacity + slack;
        Self {
            samples: vec![T::zero(); phases * stride],
            phases,
            stride,
            capacity,
            history,
            start: 0,
            filled: history,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.samples.fill(T::zero());
        self.start = 0;
        self.filled = self.history;
    }

    pub(crate) fn stride(&self) -> usize {
        self.stride
    }

    pub(crate) fn room(&self) -> usize {
        self.phases * self.capacity - self.history
    }

    pub(crate) fn len(&self) -> usize {
        self.filled - self.start * self.phases
    }

    pub(crate) fn windows(&self, span: usize) -> usize {
        let len = self.len();
        if len < span {
            0
        } else {
            (len - span) / self.phases + 1
        }
    }

    pub(crate) fn push(&mut self, chunk: &[T]) {
        if self.filled + chunk.len() > self.phases * self.capacity {
            self.compact();
        }
        if self.phases == 1 {
            self.samples[self.filled..self.filled + chunk.len()].copy_from_slice(chunk);
        } else {
            self.scatter(chunk);
        }
        self.filled += chunk.len();
    }

    fn scatter(&mut self, chunk: &[T]) {
        let lead = (self.phases - self.filled % self.phases) % self.phases;
        let (head, body) = chunk.split_at(lead.min(chunk.len()));
        let mut filled = self.filled;
        for &sample in head {
            self.put(filled, sample);
            filled += 1;
        }
        let index = filled / self.phases;
        let aligned = match self.phases {
            2 => self.transpose_2(index, body),
            3 => self.transpose::<3>(index, body),
            4 => self.transpose_4(index, body),
            5 => self.transpose::<5>(index, body),
            6 => self.transpose::<6>(index, body),
            7 => self.transpose::<7>(index, body),
            8 => self.transpose::<8>(index, body),
            9 => self.transpose::<9>(index, body),
            10 => self.transpose::<10>(index, body),
            11 => self.transpose::<11>(index, body),
            12 => self.transpose::<12>(index, body),
            13 => self.transpose::<13>(index, body),
            14 => self.transpose::<14>(index, body),
            15 => self.transpose::<15>(index, body),
            16 => self.transpose::<16>(index, body),
            _ => self.transpose_any(index, body),
        };
        let tail = &body[aligned..];
        filled += aligned;
        for &sample in tail {
            self.put(filled, sample);
            filled += 1;
        }
    }

    fn transpose_2(&mut self, index: usize, body: &[T]) -> usize {
        let (groups, _) = body.as_chunks::<2>();
        let (row0, rest) = self.samples.split_at_mut(self.stride);
        let r0 = &mut row0[index..index + groups.len()];
        let r1 = &mut rest[index..index + groups.len()];
        let done = T::deinterleave_2(groups, r0, r1);
        for (i, group) in groups[done..].iter().enumerate() {
            r0[done + i] = group[0];
            r1[done + i] = group[1];
        }
        groups.len() * 2
    }

    fn transpose_4(&mut self, index: usize, body: &[T]) -> usize {
        let (groups, _) = body.as_chunks::<4>();
        let stride = self.stride;
        let (row0, rest) = self.samples.split_at_mut(stride);
        let (row1, rest) = rest.split_at_mut(stride);
        let (row2, row3) = rest.split_at_mut(stride);
        let r0 = &mut row0[index..index + groups.len()];
        let r1 = &mut row1[index..index + groups.len()];
        let r2 = &mut row2[index..index + groups.len()];
        let r3 = &mut row3[index..index + groups.len()];
        let done = T::deinterleave_4(groups, r0, r1, r2, r3);
        for (i, group) in groups[done..].iter().enumerate() {
            r0[done + i] = group[0];
            r1[done + i] = group[1];
            r2[done + i] = group[2];
            r3[done + i] = group[3];
        }
        groups.len() * 4
    }

    fn transpose<const PHASES: usize>(&mut self, index: usize, body: &[T]) -> usize {
        let (groups, _) = body.as_chunks::<PHASES>();
        let mut rows = self
            .samples
            .chunks_exact_mut(self.stride)
            .map(|row| &mut row[index..index + groups.len()]);
        let mut rows: [&mut [T]; PHASES] = std::array::from_fn(|_| rows.next().unwrap_or_default());
        let (eights, last) = groups.as_chunks::<8>();
        for (phase, row) in rows.iter_mut().enumerate() {
            for (slot, eight) in row.as_chunks_mut::<8>().0.iter_mut().zip(eights) {
                *slot = std::array::from_fn(|at| eight[at][phase]);
            }
        }
        for (offset, group) in last.iter().enumerate() {
            for (row, &sample) in rows.iter_mut().zip(group) {
                row[8 * eights.len() + offset] = sample;
            }
        }
        groups.len() * PHASES
    }

    #[inline(never)]
    fn transpose_any(&mut self, index: usize, body: &[T]) -> usize {
        let groups = body.chunks_exact(self.phases);
        let aligned = body.len() - groups.remainder().len();
        let rows = &mut self.samples[index..];
        for (offset, group) in groups.enumerate() {
            for (phase, &sample) in group.iter().enumerate() {
                rows[phase * self.stride + offset] = sample;
            }
        }
        aligned
    }

    fn put(&mut self, position: usize, sample: T) {
        self.samples[position % self.phases * self.stride + position / self.phases] = sample;
    }

    fn compact(&mut self) {
        let end = self.filled.div_ceil(self.phases);
        for row in self.samples.chunks_exact_mut(self.stride) {
            row.copy_within(self.start..end, 0);
        }
        self.filled -= self.start * self.phases;
        self.start = 0;
    }

    pub(crate) fn rows_from(&self, at: usize) -> &[T] {
        &self.samples[self.start + at..]
    }

    pub(crate) fn consume(&mut self, windows: usize) {
        self.start += windows.min(self.len() / self.phases);
    }
}

impl DelayLine<f32> {
    pub(crate) fn push_complex_planes(
        re: &mut Self,
        im: &mut Self,
        chunk: &[num_complex::Complex<f32>],
    ) {
        debug_assert_eq!(re.phases, 1);
        debug_assert_eq!(im.phases, 1);
        if re.filled + chunk.len() > re.capacity {
            re.compact();
        }
        if im.filled + chunk.len() > im.capacity {
            im.compact();
        }
        let re_dst = &mut re.samples[re.filled..re.filled + chunk.len()];
        let im_dst = &mut im.samples[im.filled..im.filled + chunk.len()];
        let mut done = 0;
        #[cfg(target_arch = "aarch64")]
        {
            use std::arch::aarch64::{vld2q_f32, vst1q_f32};
            let n = chunk.len() & !3;
            unsafe {
                let sp = chunk.as_ptr().cast::<f32>();
                let rp = re_dst.as_mut_ptr();
                let ip = im_dst.as_mut_ptr();
                while done < n {
                    let v = vld2q_f32(sp.add(done * 2));
                    vst1q_f32(rp.add(done), v.0);
                    vst1q_f32(ip.add(done), v.1);
                    done += 4;
                }
            }
        }
        for ((r, i), sample) in re_dst[done..]
            .iter_mut()
            .zip(&mut im_dst[done..])
            .zip(&chunk[done..])
        {
            *r = sample.re;
            *i = sample.im;
        }
        re.filled += chunk.len();
        im.filled += chunk.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(len: usize) -> Vec<f32> {
        (0..len).map(|index| index as f32 + 1.0).collect()
    }

    #[test]
    fn phases_hold_every_nth_sample_across_compactions() {
        for phases in [1, 2, 3, 10, 13, 25] {
            let history = 4 * phases + 1;
            let mut line = DelayLine::<f32>::new(history, phases, 16);
            let input = stream(40_000);
            let mut seen = vec![0.0; history];
            let mut rest = input.as_slice();
            for len in [1usize, 700, 3, 5000, 64].into_iter().cycle() {
                if rest.is_empty() {
                    break;
                }
                let (chunk, tail) = rest.split_at(len.min(line.room()).min(rest.len()));
                line.push(chunk);
                seen.extend_from_slice(chunk);
                rest = tail;
                let windows = line.windows(history + 1);
                for window in 0..windows {
                    let rows = line.rows_from(window);
                    let base = seen.len() - line.len() + window * phases;
                    for offset in 0..=history {
                        let sample = rows[(offset % phases) * line.stride() + offset / phases];
                        assert_eq!(sample, seen[base + offset], "phases={phases}");
                    }
                }
                line.consume(windows);
            }
        }
    }

    #[test]
    fn reset_restores_the_zero_history() {
        let mut line = DelayLine::<f32>::new(9, 2, 16);
        line.push(&stream(line.room()));
        line.consume(line.windows(10));
        line.reset();
        assert_eq!(line.len(), 9);
        assert_eq!(line.windows(10), 0);
        assert!(line.rows_from(0).iter().all(|&sample| sample == 0.0));
    }
}
