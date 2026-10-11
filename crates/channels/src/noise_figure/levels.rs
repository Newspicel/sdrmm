pub(crate) const STEP_DB: f64 = 0.5;
pub(crate) const MIN_SETTLED_BLOCKS: u32 = 5;

#[derive(Clone, Copy, Debug, Default)]
struct Segment {
    sum: f64,
    blocks: u32,
}

impl Segment {
    fn add(&mut self, power: f64) {
        self.sum += power;
        self.blocks += 1;
    }

    fn mean(&self) -> Option<f64> {
        (self.blocks > 0).then(|| self.sum / f64::from(self.blocks))
    }

    fn settled(&self) -> Option<f64> {
        (self.blocks >= MIN_SETTLED_BLOCKS)
            .then(|| self.mean())
            .flatten()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Pair {
    pub hot: f64,
    pub cold: f64,
}

impl Pair {
    fn of(a: f64, b: f64) -> Option<Self> {
        apart(a, b).then(|| Self {
            hot: a.max(b),
            cold: a.min(b),
        })
    }

    #[must_use]
    pub fn y(&self) -> f64 {
        self.hot / self.cold
    }
}

fn db(power: f64) -> f64 {
    10.0 * power.log10()
}

fn apart(a: f64, b: f64) -> bool {
    (db(a) - db(b)).abs() > STEP_DB
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Levels {
    open: Segment,
    outlier: bool,
    last: Option<f64>,
    pair: Option<Pair>,
}

impl Levels {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn block(&mut self, power: f64) {
        if !(power.is_finite() && power > 0.0) {
            return;
        }
        match self.open.mean() {
            Some(mean) if apart(power, mean) => {
                if self.outlier {
                    self.close();
                    self.open.add(power);
                } else {
                    self.outlier = true;
                }
            }
            _ => {
                self.outlier = false;
                self.open.add(power);
            }
        }
    }

    fn close(&mut self) {
        if let Some(level) = self.open.settled() {
            if let Some(pair) = self.last.and_then(|last| Pair::of(level, last)) {
                self.pair = Some(pair);
            }
            self.last = Some(level);
        }
        self.open = Segment::default();
        self.outlier = false;
    }

    #[must_use]
    pub fn current(&self) -> Option<f64> {
        self.open.mean()
    }

    #[must_use]
    pub fn settled_blocks(&self) -> u32 {
        self.open.blocks
    }

    #[must_use]
    pub fn pair(&self) -> Option<Pair> {
        self.open
            .settled()
            .zip(self.last)
            .and_then(|(open, last)| Pair::of(open, last))
            .or(self.pair)
    }

    #[must_use]
    pub fn source_on(&self) -> Option<bool> {
        let open = self.open.settled()?;
        let pair = self.pair()?;
        if !apart(open, pair.hot) {
            Some(true)
        } else if !apart(open, pair.cold) {
            Some(false)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(levels: &mut Levels, power: f64, blocks: u32) {
        (0..blocks).for_each(|_| levels.block(power));
    }

    #[test]
    fn one_level_has_no_pair() {
        let mut levels = Levels::default();
        feed(&mut levels, 1.0, 50);
        assert_eq!(levels.pair(), None);
        assert_eq!(levels.source_on(), None);
    }

    #[test]
    fn a_step_pairs_the_live_level_with_the_last_one() {
        let mut levels = Levels::default();
        feed(&mut levels, 1.0, 20);
        feed(&mut levels, 10.0, 3);
        assert_eq!(levels.pair(), None);
        feed(&mut levels, 10.0, 10);
        assert_eq!(
            levels.pair(),
            Some(Pair {
                hot: 10.0,
                cold: 1.0
            })
        );
        assert_eq!(levels.source_on(), Some(true));
        feed(&mut levels, 1.0, 10);
        assert_eq!(levels.source_on(), Some(false));
        assert!((levels.pair().unwrap().y() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn a_lone_outlier_does_not_split_a_level() {
        let mut levels = Levels::default();
        feed(&mut levels, 1.0, 10);
        levels.block(50.0);
        feed(&mut levels, 1.0, 10);
        assert_eq!(levels.settled_blocks(), 20);
        assert_eq!(levels.current(), Some(1.0));
    }

    #[test]
    fn a_short_level_is_not_kept() {
        let mut levels = Levels::default();
        feed(&mut levels, 1.0, 20);
        feed(&mut levels, 10.0, 3);
        feed(&mut levels, 1.0, 3);
        assert_eq!(levels.pair(), None);
    }

    #[test]
    fn non_positive_power_is_ignored() {
        let mut levels = Levels::default();
        levels.block(0.0);
        levels.block(f64::NAN);
        assert_eq!(levels.current(), None);
    }
}
