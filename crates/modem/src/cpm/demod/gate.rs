use sdrmm_dsp::one_pole_coeff;

const ENVELOPE_TAU_SYMBOLS: f64 = 0.5;

const FLOOR_TAU_SYMBOLS: f64 = 96.0;

const FLOOR_SETTLE: f64 = 4.0;

pub(super) const CARRIER_RISE: f32 = 4.0;

const STEADY_SPREAD: f32 = 0.25;

/// How long the envelope must stay under the carrier threshold before the gate closes. Near
/// the noise floor a burst's envelope dips under it for a sample or two; without a hangover each
/// dip restarted the settle and cut the burst into pieces too short to time.
const HANGOVER_SYMBOLS: f64 = 2.0;

/// The share of noise's own wild rate at which an open gate is taken to hold noise, and its floor
/// follows so a noise floor that rose since it was learned closes the gate again. A carrier
/// a few dB over the noise runs at under half of it.
const NOISE_LIKENESS: f32 = 0.75;

/// How far under the level it was keyed at the envelope must fall for a carrier to have ended.
/// The gate then only keys again on something clearly over what is left, however much higher
/// that is than the floor learned before.
const CARRIER_DROP: f32 = 16.0;

/// How much faster than usual the floor rises once an open gate is known to hold noise.
const NOISE_FLOOR_SPEED: f32 = 8.0;

/// How quickly an open gate's wild rate follows the input.
const WILD_TAU_SYMBOLS: f64 = 16.0;

#[derive(Clone, Copy)]
pub(super) struct Gate {
    pub(super) envelope: f32,
    pub(super) floor: f32,
    steady_level: f32,
    mean: f32,
    mean_square: f32,
    envelope_alpha: f32,
    floor_alpha: f32,
    one_minus_floor_alpha: f32,
    noise_floor_alpha: f32,
    settling: usize,
    pub(super) settle_samples: usize,
    pub(super) keyed: usize,
    pub(super) support: usize,
    quiet: usize,
    hangover: usize,
    carrier_level: f32,
    wild_rate: f32,
    noise_wild: f32,
    noise_heard: usize,
    wild_alpha: f32,
    pub(super) always_keyed: bool,
}

impl Gate {
    pub(super) fn new(sps: f64, support: usize) -> Self {
        let settle = (FLOOR_SETTLE * FLOOR_TAU_SYMBOLS * sps) as usize;
        let floor_alpha = one_pole_coeff(sps, FLOOR_TAU_SYMBOLS);
        Self {
            envelope: 0.0,
            floor: 0.0,
            steady_level: 0.0,
            mean: 0.0,
            mean_square: 0.0,
            envelope_alpha: one_pole_coeff(sps, ENVELOPE_TAU_SYMBOLS),
            floor_alpha,
            one_minus_floor_alpha: 1.0 - floor_alpha,
            noise_floor_alpha: NOISE_FLOOR_SPEED * floor_alpha,
            settling: settle,
            settle_samples: settle,
            keyed: 0,
            support,
            quiet: 0,
            hangover: (HANGOVER_SYMBOLS * sps).round() as usize,
            carrier_level: 0.0,
            wild_rate: 0.0,
            noise_wild: 0.0,
            noise_heard: 0,
            wild_alpha: one_pole_coeff(sps, WILD_TAU_SYMBOLS),
            always_keyed: false,
        }
    }

    /// `wild` says whether the discriminator swung past the signal's deviation on this sample,
    /// when the front has a discriminator to ask.
    #[inline]
    pub(super) fn step(&mut self, power: f32, wild: Option<bool>) -> (bool, bool) {
        if self.always_keyed {
            self.keyed = (self.keyed + 1).min(self.support);
            return (true, self.keyed == self.support);
        }
        self.envelope += self.envelope_alpha * (power - self.envelope);
        self.mean += self.floor_alpha * (power - self.mean);
        self.mean_square += self.floor_alpha * (power * power - self.mean_square);
        self.settling = self.settling.saturating_sub(1);
        let spread = self.mean_square - self.mean * self.mean;
        let steady = spread < STEADY_SPREAD * self.mean * self.mean;
        let carrier = self.settling == 0 && (steady || self.envelope > self.floor * CARRIER_RISE);
        self.quiet = if carrier { 0 } else { self.quiet + 1 };
        let keyed = carrier || (self.keyed > 0 && self.quiet <= self.hangover);
        let noisy = wild.is_some_and(|wild| self.hears_noise(wild, keyed));
        if steady {
            self.steady_level = self.mean;
        }
        self.carrier_level = if keyed && !noisy {
            self.envelope
                .max(self.carrier_level * self.one_minus_floor_alpha)
        } else {
            0.0
        };
        if self.steady_level > 0.0 && self.mean * CARRIER_RISE < self.steady_level {
            self.floor = self.mean;
            self.steady_level = 0.0;
        } else if self.carrier_level > 0.0 && self.envelope * CARRIER_DROP < self.carrier_level {
            self.floor = self.floor.max(self.envelope * (1.0 / CARRIER_RISE));
            self.carrier_level = 0.0;
        } else if noisy {
            self.floor += self.noise_floor_alpha * (self.envelope - self.floor);
        } else if !keyed && !steady {
            self.floor += self.floor_alpha * (self.envelope - self.floor);
        }
        self.keyed = if keyed {
            (self.keyed + 1).min(self.support)
        } else {
            0
        };
        (keyed, self.keyed == self.support && !noisy)
    }

    /// Whether an open gate's input swings like noise, learning noise's wild rate while closed.
    #[inline(always)]
    fn hears_noise(&mut self, wild: bool, keyed: bool) -> bool {
        let wild = f32::from(u8::from(wild));
        if !keyed {
            self.noise_wild += self.floor_alpha * (wild - self.noise_wild);
            self.noise_heard = self.noise_heard.saturating_add(1);
            self.wild_rate = 0.0;
            return false;
        }
        self.wild_rate += self.wild_alpha * (wild - self.wild_rate);
        self.noise_heard >= self.settle_samples && self.wild_rate > NOISE_LIKENESS * self.noise_wild
    }

    pub(super) fn reset(&mut self) {
        self.envelope = 0.0;
        self.floor = 0.0;
        self.steady_level = 0.0;
        self.mean = 0.0;
        self.mean_square = 0.0;
        self.settling = self.settle_samples;
        self.keyed = 0;
        self.quiet = 0;
        self.carrier_level = 0.0;
        self.wild_rate = 0.0;
        self.noise_wild = 0.0;
        self.noise_heard = 0;
    }
}
