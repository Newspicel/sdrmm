/// Taps whose sample offsets step evenly, so a kernel walks them with a pointer instead of
/// loading an offset for every tap. Folded runs step their mirror the other way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Run {
    pub(super) front: usize,
    pub(super) back: usize,
    pub(super) step: usize,
    pub(super) start: usize,
    pub(super) len: usize,
}

impl Run {
    pub(super) fn range(&self) -> std::ops::Range<usize> {
        self.start..self.start + self.len
    }

    pub(super) fn offsets(&self, at: usize) -> (usize, usize) {
        let shift = at * self.step;
        (self.front + shift, self.back.wrapping_sub(shift))
    }

    fn takes(&mut self, front: usize, back: usize, folded: bool) -> bool {
        if self.len == 1 {
            let Some(step) = front.checked_sub(self.front) else {
                return false;
            };
            if folded && self.back.checked_sub(back) != Some(step) {
                return false;
            }
            self.step = step;
        } else {
            let (next_front, next_back) = self.offsets(self.len);
            if front != next_front || (folded && back != next_back) {
                return false;
            }
        }
        self.len += 1;
        true
    }
}

/// Groups the nonzero taps of each phase into runs; a zero tap costs nothing.
pub(super) fn runs<C: Copy + PartialEq + Default>(
    count: usize,
    phases: usize,
    folded: bool,
    tap: impl Fn(usize) -> (usize, usize, C),
) -> (Vec<Run>, Vec<C>) {
    let mut runs = Vec::new();
    let mut values = Vec::new();
    for phase in 0..phases.min(count) {
        let mut open: Option<Run> = None;
        for index in (phase..count).step_by(phases) {
            let (front, back, value) = tap(index);
            if value == C::default() {
                continue;
            }
            let extended = open
                .as_mut()
                .is_some_and(|run| run.takes(front, back, folded));
            if !extended {
                runs.extend(open.take());
                open = Some(Run {
                    front,
                    back,
                    step: 0,
                    start: values.len(),
                    len: 1,
                });
            }
            values.push(value);
        }
        runs.extend(open);
    }
    (runs, values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_halfband_by_two_is_one_dense_run_and_its_centre() {
        let taps = [1.0f32, 0.0, 2.0, 0.0, 3.0, 5.0, 3.0, 0.0, 2.0, 0.0, 1.0];
        let last = taps.len() - 1;
        let offset = |index: usize| (index % 2) * 100 + index / 2;
        let (runs, values) = runs(6, 2, true, |index| {
            (offset(index), offset(last - index), taps[index])
        });
        assert_eq!(values, [1.0, 2.0, 3.0, 5.0]);
        assert_eq!(runs.len(), 2);
        assert_eq!(
            (runs[0].front, runs[0].back, runs[0].step, runs[0].len),
            (0, 5, 1, 3)
        );
        assert_eq!((runs[1].front, runs[1].len), (102, 1));
    }

    #[test]
    fn an_interior_zero_without_a_pattern_splits_the_run() {
        let taps = [1.0f32, 2.0, 0.0, 4.0, 5.0, 6.0];
        let (runs, values) = runs(taps.len(), 1, false, |index| (index, index, taps[index]));
        assert_eq!(values, [1.0, 2.0, 4.0, 5.0, 6.0]);
        assert_eq!(runs.iter().map(|run| run.len).collect::<Vec<_>>(), [2, 3]);
        let every_other = [1.0f32, 0.0, 2.0, 0.0, 3.0];
        let (runs, _) = super::runs(5, 1, false, |index| (index, index, every_other[index]));
        assert_eq!((runs.len(), runs[0].step, runs[0].len), (1, 2, 3));
    }
}
