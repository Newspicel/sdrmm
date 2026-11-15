use super::error::OfdmError;

const MIN_FFT: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Subcarrier {
    pub bin: usize,
    pub offset: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubcarrierMap {
    fft: usize,
    data: Vec<Subcarrier>,
    pilots: Vec<Subcarrier>,
    occupied: Vec<Subcarrier>,
}

impl SubcarrierMap {
    pub fn new(fft: usize, data: &[i32], pilots: &[i32]) -> Result<Self, OfdmError> {
        if !fft.is_power_of_two() || fft < MIN_FFT {
            return Err(OfdmError::FftSize(fft));
        }
        if data.is_empty() {
            return Err(OfdmError::NoDataSubcarriers);
        }
        let half = (fft / 2) as i32;
        let mut seen = vec![false; fft];
        let mut carrier = |offset: i32| -> Result<Subcarrier, OfdmError> {
            if offset < -half || offset >= half {
                return Err(OfdmError::SubcarrierOutOfRange { offset, fft });
            }
            let bin = offset.rem_euclid(fft as i32) as usize;
            if std::mem::replace(&mut seen[bin], true) {
                return Err(OfdmError::DuplicateSubcarrier(offset));
            }
            Ok(Subcarrier { bin, offset })
        };
        let mut built_data: Vec<Subcarrier> =
            data.iter().map(|&k| carrier(k)).collect::<Result<_, _>>()?;
        let mut built_pilots: Vec<Subcarrier> = pilots
            .iter()
            .map(|&k| carrier(k))
            .collect::<Result<_, _>>()?;
        built_data.sort_unstable_by_key(|c| c.offset);
        built_pilots.sort_unstable_by_key(|c| c.offset);
        let mut occupied = built_data.clone();
        occupied.extend_from_slice(&built_pilots);
        occupied.sort_unstable_by_key(|c| c.offset);
        Ok(Self {
            fft,
            data: built_data,
            pilots: built_pilots,
            occupied,
        })
    }

    #[must_use]
    pub fn fft(&self) -> usize {
        self.fft
    }

    #[must_use]
    pub fn data(&self) -> &[Subcarrier] {
        &self.data
    }

    #[must_use]
    pub fn pilots(&self) -> &[Subcarrier] {
        &self.pilots
    }

    #[must_use]
    pub fn occupied(&self) -> &[Subcarrier] {
        &self.occupied
    }

    pub(super) fn is_hermitian_capable(&self) -> Result<(), OfdmError> {
        let half = (self.fft / 2) as i32;
        for c in &self.occupied {
            if c.offset <= 0 || c.offset >= half {
                return Err(OfdmError::NotHermitian(c.offset));
            }
        }
        Ok(())
    }
}
