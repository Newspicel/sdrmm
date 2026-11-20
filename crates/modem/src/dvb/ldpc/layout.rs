use sdrmm_dsp::coding::fec::ldpc::{Circulant, QcLayout};

use super::GROUP;

/// DVB-S2's parity bits, read column-wise into rows of 360 so every check row of the
/// standard's address table becomes one circulant layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Layout {
    pub information: usize,
    pub layers: usize,
}

impl Layout {
    pub(super) fn build(length: usize, addresses: &[&[u16]]) -> Option<(Self, QcLayout)> {
        let information = addresses.len() * GROUP;
        let parity = length.checked_sub(information)?;
        let layers = parity / GROUP;
        if information == 0 || layers < 2 || !parity.is_multiple_of(GROUP) {
            return None;
        }
        if addresses
            .iter()
            .any(|row| row.is_empty() || row.iter().any(|&address| usize::from(address) >= parity))
        {
            return None;
        }
        let mut grouped: Vec<Vec<Circulant>> = vec![Vec::new(); layers];
        for (group, row) in addresses.iter().enumerate() {
            for &address in *row {
                let address = usize::from(address);
                grouped[address % layers].push(Circulant {
                    base: group * GROUP,
                    shift: (GROUP - address / layers) % GROUP,
                    open: false,
                });
            }
        }
        for (layer, circulants) in grouped.iter_mut().enumerate() {
            circulants.extend(staircase(information, layers, layer));
        }
        let qc = QcLayout::new(GROUP, length, grouped)?;
        Some((
            Self {
                information,
                layers,
            },
            qc,
        ))
    }

    /// The parity check the standard numbers for row `lane` of `layer`.
    pub(super) const fn check(self, layer: usize, lane: usize) -> usize {
        layer + self.layers * lane
    }

    #[cfg(test)]
    /// Where transmitted bit `bit` sits among the decoder's variables.
    pub(super) const fn position(self, bit: usize) -> usize {
        if bit < self.information {
            bit
        } else {
            let parity = bit - self.information;
            self.information + (parity % self.layers) * GROUP + parity / self.layers
        }
    }
}

fn staircase(information: usize, layers: usize, layer: usize) -> [Circulant; 2] {
    let own = information + layer * GROUP;
    let previous = if layer == 0 {
        Circulant {
            base: information + (layers - 1) * GROUP,
            shift: GROUP - 1,
            open: true,
        }
    } else {
        Circulant {
            base: own - GROUP,
            shift: 0,
            open: false,
        }
    };
    [
        Circulant {
            base: own,
            shift: 0,
            open: false,
        },
        previous,
    ]
}
