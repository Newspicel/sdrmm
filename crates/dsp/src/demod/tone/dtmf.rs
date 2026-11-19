use super::goertzel::Goertzel;

/// The low group of ITU-T Q.23, one tone per keypad row.
pub const DTMF_ROW_HZ: [f64; 4] = [697.0, 770.0, 852.0, 941.0];
/// The high group of ITU-T Q.23, one tone per keypad column.
pub const DTMF_COLUMN_HZ: [f64; 4] = [1_209.0, 1_336.0, 1_477.0, 1_633.0];

const SYMBOLS: [char; 16] = [
    '1', '2', '3', 'A', '4', '5', '6', 'B', '7', '8', '9', 'C', '*', '0', '#', 'D',
];

/// 12.75 ms, the 102 samples at 8 kHz spandsp settled on: adjacent row tones fall near each
/// other's first null, yet a 20 ms tone still fills a whole window.
const WINDOW_S: f64 = 0.012_75;
const HOP_S: f64 = 0.002;
/// About -40 dBFS per tone, under which a window counts as silence.
const MIN_TONE_POWER: f32 = 1e-4;
/// Q.24 lets the groups differ by a few dB on a line; FM pre- and de-emphasis tilt them
/// further, so either group may lead the other by up to 10 dB.
const MAX_TWIST: f32 = 10.0;
/// Every other tone of a group sits at least 8 dB under the group's strongest.
const RELATIVE_PEAK: f32 = 6.3;
/// The two tones carry at least half the window's AC power, which rejects speech and noise
/// and makes a tone's run of windows about as long as the tone itself.
const TONE_SHARE: f64 = 0.5;

/// One of the 16 keys of the Q.23 keypad.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DtmfKey(u8);

impl DtmfKey {
    #[must_use]
    pub fn from_symbol(symbol: char) -> Option<Self> {
        let upper = symbol.to_ascii_uppercase();
        SYMBOLS
            .iter()
            .position(|&known| known == upper)
            .and_then(|index| u8::try_from(index).ok())
            .map(Self)
    }

    #[must_use]
    pub const fn symbol(self) -> char {
        SYMBOLS[self.0 as usize]
    }

    /// The row (low group) and column (high group) tones the key sends.
    #[must_use]
    pub const fn tones_hz(self) -> (f64, f64) {
        (
            DTMF_ROW_HZ[(self.0 / 4) as usize],
            DTMF_COLUMN_HZ[(self.0 % 4) as usize],
        )
    }

    const fn at(row: usize, column: usize) -> Self {
        Self((row * 4 + column) as u8)
    }
}

/// How long a tone must last to be a key press, and how long a break inside it may last
/// before the press ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DtmfTiming {
    pub min_tone_s: f64,
    pub max_break_s: f64,
}

impl DtmfTiming {
    /// ITU-T Q.24: a 40 ms tone must register and a 20 ms one must not, so the threshold sits
    /// between them; breaks up to 10 ms are bridged.
    pub const Q24: Self = Self {
        min_tone_s: 0.030,
        max_break_s: 0.010,
    };

    /// Fast encoders that key 20 ms tones with 5 ms gaps; noise and the neighbouring tones
    /// leave as little as 12 ms of such a tone clean.
    pub const FAST: Self = Self {
        min_tone_s: 0.010,
        max_break_s: 0.003,
    };
}

/// A key held for `start..end`, in samples since the detector began.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DtmfPress {
    pub key: DtmfKey,
    pub start: u64,
    pub end: u64,
}

impl DtmfPress {
    #[must_use]
    pub const fn samples(&self) -> u64 {
        self.end - self.start
    }
}

/// Finds DTMF key presses in audio: a Goertzel bank over a sliding window checks each hop for
/// one row and one column tone with a sane twist and nothing else, then a press needs the
/// same key for [`DtmfTiming::min_tone_s`].
#[derive(Clone, Debug)]
pub struct DtmfDetector {
    history: Vec<f32>,
    pos: usize,
    bank: [Goertzel; 8],
    power: [f32; 8],
    hop: usize,
    since_hop: usize,
    clock: u64,
    min_tone: u64,
    max_misses: u32,
    held: Option<DtmfPress>,
    pending: Option<DtmfPress>,
    misses: u32,
}

impl DtmfDetector {
    #[must_use]
    pub fn new(sample_rate: f64, timing: DtmfTiming) -> Self {
        assert!(
            sample_rate >= 4_000.0,
            "DTMF needs a sample rate of at least 4 kHz"
        );
        let window = (sample_rate * WINDOW_S).round() as usize;
        let hop = (sample_rate * HOP_S).round().max(1.0) as usize;
        let tones: [f64; 8] = std::array::from_fn(|index| {
            if index < 4 {
                DTMF_ROW_HZ[index]
            } else {
                DTMF_COLUMN_HZ[index - 4]
            }
        });
        Self {
            history: vec![0.0; window],
            pos: 0,
            bank: tones.map(|hz| Goertzel::new(sample_rate, hz, window)),
            power: [0.0; 8],
            hop,
            since_hop: 0,
            clock: 0,
            min_tone: (sample_rate * timing.min_tone_s).round() as u64,
            max_misses: (timing.max_break_s / HOP_S).round() as u32,
            held: None,
            pending: None,
            misses: 0,
        }
    }

    pub fn reset(&mut self) {
        self.history.fill(0.0);
        self.pos = 0;
        self.since_hop = 0;
        self.held = None;
        self.pending = None;
        self.misses = 0;
    }

    /// Samples taken so far, the clock [`DtmfPress`] counts in.
    #[must_use]
    pub const fn clock(&self) -> u64 {
        self.clock
    }

    /// A key is down now, or may still turn out to be.
    #[must_use]
    pub const fn busy(&self) -> bool {
        self.held.is_some() || self.pending.is_some()
    }

    /// Takes one sample; returns a press once its key has been released.
    pub fn push(&mut self, sample: f32) -> Option<DtmfPress> {
        self.history[self.pos] = if sample.is_finite() { sample } else { 0.0 };
        self.pos = (self.pos + 1) % self.history.len();
        self.clock += 1;
        self.since_hop += 1;
        if self.since_hop < self.hop {
            return None;
        }
        self.since_hop = 0;
        let key = self.classify();
        self.step(key)
    }

    fn classify(&mut self) -> Option<DtmfKey> {
        let (mut sum, mut squares) = (0.0f64, 0.0f64);
        let (recent, oldest) = self.history.split_at(self.pos);
        for &sample in oldest.iter().chain(recent) {
            sum += f64::from(sample);
            squares += f64::from(sample) * f64::from(sample);
            for (goertzel, power) in self.bank.iter_mut().zip(&mut self.power) {
                if let Some(level) = goertzel.push(sample) {
                    *power = level;
                }
            }
        }
        let len = self.history.len() as f64;
        let ac_power = (squares - sum * sum / len) / len;
        let (rows, columns) = self.power.split_at(4);
        let row = strongest(rows)?;
        let column = strongest(columns)?;
        let (low, high) = (rows[row], columns[column]);
        let tone_power = f64::from(low + high) / 2.0;
        let balanced = high <= low * MAX_TWIST && low <= high * MAX_TWIST;
        (low >= MIN_TONE_POWER
            && high >= MIN_TONE_POWER
            && balanced
            && tone_power >= TONE_SHARE * ac_power)
            .then_some(DtmfKey::at(row, column))
    }

    /// The decision this hop stands for covers `hop` samples around its window's centre.
    fn decision(&self, key: DtmfKey) -> DtmfPress {
        let half_window = self.history.len() as u64 / 2;
        let start = self.clock.saturating_sub(half_window + self.hop as u64 / 2);
        DtmfPress {
            key,
            start,
            end: start + self.hop as u64,
        }
    }

    fn step(&mut self, key: Option<DtmfKey>) -> Option<DtmfPress> {
        let now = key.map(|key| self.decision(key));
        if let (Some(held), Some(now)) = (self.held.as_mut(), now)
            && held.key == now.key
        {
            held.end = now.end;
            self.misses = 0;
            self.pending = None;
            return None;
        }
        self.pending = match (self.pending, now) {
            (Some(mut pending), Some(now)) if pending.key == now.key => {
                pending.end = now.end;
                Some(pending)
            }
            (_, now) => now,
        };
        self.misses += 1;
        if self.held.is_some() && self.misses <= self.max_misses {
            return None;
        }
        let released = self.held.take();
        self.held = self.pending.take();
        self.misses = 0;
        released.filter(|press| press.samples() >= self.min_tone)
    }
}

/// The strongest tone of a group, when every other one sits [`RELATIVE_PEAK`] under it.
fn strongest(powers: &[f32]) -> Option<usize> {
    let (best, &peak) = powers
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))?;
    powers
        .iter()
        .enumerate()
        .all(|(index, &power)| index == best || power * RELATIVE_PEAK <= peak)
        .then_some(best)
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use super::*;

    const RATE: f64 = 48_000.0;

    fn samples(seconds: f64) -> usize {
        (RATE * seconds).round() as usize
    }

    fn key_audio(key: DtmfKey, seconds: f64, low_amp: f64, high_amp: f64) -> Vec<f32> {
        let (low, high) = key.tones_hz();
        (0..samples(seconds))
            .map(|n| {
                let t = n as f64 / RATE;
                (low_amp * (TAU * low * t).sin() + high_amp * (TAU * high * t).sin()) as f32
            })
            .collect()
    }

    fn dialed(symbols: &str, tone_s: f64, gap_s: f64) -> Vec<f32> {
        let mut audio = vec![0.0; samples(0.05)];
        for symbol in symbols.chars() {
            let key = DtmfKey::from_symbol(symbol).expect("a keypad symbol");
            audio.extend(key_audio(key, tone_s, 0.3, 0.3));
            audio.resize(audio.len() + samples(gap_s), 0.0);
        }
        audio.resize(audio.len() + samples(0.1), 0.0);
        audio
    }

    fn presses(audio: &[f32], timing: DtmfTiming) -> Vec<DtmfPress> {
        let mut detector = DtmfDetector::new(RATE, timing);
        audio.iter().filter_map(|&s| detector.push(s)).collect()
    }

    fn keys(presses: &[DtmfPress]) -> String {
        presses.iter().map(|press| press.key.symbol()).collect()
    }

    #[test]
    fn the_keypad_holds_the_q23_tones() {
        let published = [
            ('1', 697.0, 1_209.0),
            ('5', 770.0, 1_336.0),
            ('9', 852.0, 1_477.0),
            ('D', 941.0, 1_633.0),
            ('*', 941.0, 1_209.0),
            ('#', 941.0, 1_477.0),
            ('0', 941.0, 1_336.0),
            ('A', 697.0, 1_633.0),
        ];
        for (symbol, low, high) in published {
            let key = DtmfKey::from_symbol(symbol).expect("a keypad symbol");
            assert_eq!(key.tones_hz(), (low, high), "{symbol}");
            assert_eq!(key.symbol(), symbol);
        }
        assert_eq!(DtmfKey::from_symbol('x'), None);
    }

    #[test]
    fn all_sixteen_keys_decode_at_q24_timing() {
        let all = "123A456B789C*0#D";
        assert_eq!(
            keys(&presses(&dialed(all, 0.04, 0.04), DtmfTiming::Q24)),
            all
        );
    }

    #[test]
    fn a_press_lasts_as_long_as_its_tone() {
        let found = presses(&dialed("7", 0.1, 0.1), DtmfTiming::Q24);
        let [press] = found.as_slice() else {
            panic!("expected one press, got {found:?}");
        };
        let ms = press.samples() as f64 / RATE * 1_000.0;
        assert!((ms - 100.0).abs() < 6.0, "measured {ms} ms");
        let start_ms = press.start as f64 / RATE * 1_000.0;
        assert!((start_ms - 50.0).abs() < 6.0, "starts at {start_ms} ms");
    }

    #[test]
    fn q24_rejects_a_20_ms_tone_that_fast_timing_takes() {
        let audio = dialed("1646347904", 0.02, 0.005);
        assert!(presses(&audio, DtmfTiming::Q24).is_empty());
        assert_eq!(keys(&presses(&audio, DtmfTiming::FAST)), "1646347904");
    }

    #[test]
    fn a_repeated_key_needs_a_real_gap() {
        assert_eq!(
            keys(&presses(&dialed("333", 0.04, 0.02), DtmfTiming::Q24)),
            "333"
        );
        let mut broken = key_audio(DtmfKey::from_symbol('3').expect("3"), 0.05, 0.3, 0.3);
        let tail = broken.split_off(samples(0.025));
        broken.resize(broken.len() + samples(0.005), 0.0);
        broken.extend(tail);
        broken.resize(broken.len() + samples(0.1), 0.0);
        assert_eq!(keys(&presses(&broken, DtmfTiming::Q24)), "3");
    }

    #[test]
    fn twist_beyond_ten_db_is_refused() {
        let key = DtmfKey::from_symbol('5').expect("5");
        let mut tilted = key_audio(key, 0.1, 0.3, 0.3 * 10f64.powf(-6.0 / 20.0));
        tilted.resize(tilted.len() + samples(0.1), 0.0);
        assert_eq!(keys(&presses(&tilted, DtmfTiming::Q24)), "5");
        let mut skewed = key_audio(key, 0.1, 0.3, 0.3 * 10f64.powf(-14.0 / 20.0));
        skewed.resize(skewed.len() + samples(0.1), 0.0);
        assert!(presses(&skewed, DtmfTiming::Q24).is_empty());
    }

    #[test]
    fn a_single_tone_or_a_chord_of_three_is_not_a_key() {
        let one: Vec<f32> = (0..samples(0.2))
            .map(|n| (0.3 * (TAU * 770.0 * n as f64 / RATE).sin()) as f32)
            .collect();
        assert!(presses(&one, DtmfTiming::Q24).is_empty());
        let key = DtmfKey::from_symbol('1').expect("1");
        let mut three = key_audio(key, 0.2, 0.3, 0.3);
        for (n, sample) in three.iter_mut().enumerate() {
            *sample += (0.3 * (TAU * 1_477.0 * n as f64 / RATE).sin()) as f32;
        }
        assert!(presses(&three, DtmfTiming::Q24).is_empty());
    }

    #[test]
    fn tones_buried_in_noise_do_not_key() {
        let mut state = 0x1234_5678u32;
        let noise: Vec<f32> = (0..samples(1.0))
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (f64::from(state) / f64::from(u32::MAX) - 0.5) as f32
            })
            .collect();
        assert!(presses(&noise, DtmfTiming::FAST).is_empty());
        let key = DtmfKey::from_symbol('8').expect("8");
        let buried: Vec<f32> = key_audio(key, 1.0, 0.05, 0.05)
            .iter()
            .zip(&noise)
            .map(|(tone, noise)| tone + 2.0 * noise)
            .collect();
        assert!(presses(&buried, DtmfTiming::FAST).is_empty());
    }
}
