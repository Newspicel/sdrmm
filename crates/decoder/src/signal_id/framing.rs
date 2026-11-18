use num_complex::Complex;
use sdrmm_dsp::{Ddc, hamming_distance};
use sdrmm_modem::cpm::{CpmDemod, TIMING_BW_BURST};
use sdrmm_wire::{Modulation, ProtocolMatch};

use super::detect::Band;
use crate::ChannelError;
use crate::catalog::{Catalog, SYNC_SEARCH_RATE_HZ, SyncSearch};

const SEARCH_SECONDS: f64 = 0.55;

const SETTLE: usize = 96;

const DEMOTION: f32 = 0.4;

const CHANCE_MARGIN: f64 = 3.0;

const PROBE_CHANCE_MARGIN: f64 = 10.0;

const PROBE_MIN_HITS: u32 = 3;

const SHORTEST_FRAME_SYMBOLS: usize = 96;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Rigour {
    Candidate,
    Probe,
}

pub struct Probe {
    pub matches: Vec<ProtocolMatch>,
    pub modulation: Modulation,
}

pub fn probe(catalog: &Catalog, iq: &[Complex<f32>], rate: f64, band: &Band) -> Option<Probe> {
    let mut candidates: Vec<ProtocolMatch> = catalog
        .syncs()
        .iter()
        .map(|mode| ProtocolMatch {
            name: mode.name.clone(),
            type_id: Some(mode.type_id.clone()),
            score: 0.0,
            confirmed: false,
            why: String::new(),
        })
        .collect();
    confirm_with(catalog, &mut candidates, iq, rate, band, Rigour::Probe);
    candidates.retain(|candidate| candidate.confirmed);
    let first = candidates.first()?;
    let levels = catalog
        .syncs()
        .iter()
        .find(|mode| mode.name == first.name)
        .map_or(4, |mode| mode.params.mapping().m());
    Some(Probe {
        matches: candidates,
        modulation: if levels == 4 {
            Modulation::Fsk4
        } else {
            Modulation::Fsk2
        },
    })
}

pub fn confirm(
    catalog: &Catalog,
    candidates: &mut [ProtocolMatch],
    iq: &[Complex<f32>],
    rate: f64,
    band: &Band,
) {
    confirm_with(catalog, candidates, iq, rate, band, Rigour::Candidate);
}

fn confirm_with(
    catalog: &Catalog,
    candidates: &mut [ProtocolMatch],
    iq: &[Complex<f32>],
    rate: f64,
    band: &Band,
    rigour: Rigour,
) {
    let searchable: Vec<(usize, &SyncSearch)> = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            let mode = catalog
                .syncs()
                .iter()
                .find(|mode| candidate.name == mode.name)?;
            Some((index, mode))
        })
        .collect();
    if searchable.is_empty() {
        return;
    }
    let baseband = match baseband(iq, rate, band) {
        Ok(samples) => samples,
        Err(error) => {
            for &(index, mode) in &searchable {
                candidates[index].why = format!("no {} frame search here: {error}", mode.name);
            }
            return;
        }
    };
    if baseband.len() <= SETTLE * 2 {
        return;
    }

    let mut confirmed_any = false;
    let mut soft = Vec::new();
    for group in group_by_waveform(&searchable) {
        let Some(&(_, reference)) = group.first() else {
            continue;
        };
        let stagger = (reference.params.sps() / 2.0) as usize;
        for offset in [0, stagger] {
            if offset >= baseband.len() {
                continue;
            }
            let mut demod = CpmDemod::new(
                &reference.params,
                &reference.receive_filter,
                TIMING_BW_BURST,
            );
            demod.process(&baseband[offset..], &mut soft);
            if soft.len() <= SETTLE {
                continue;
            }
            for &(index, mode) in &group {
                if plausible(
                    mode,
                    hits(&soft[SETTLE..], mode),
                    soft.len() - SETTLE,
                    rigour,
                ) {
                    candidates[index].confirmed = true;
                    candidates[index].score = 1.0;
                    candidates[index].why = format!("{} frame sync found in the signal", mode.name);
                    confirmed_any = true;
                }
            }
        }
    }

    if confirmed_any {
        for (index, _) in searchable {
            if !candidates[index].confirmed {
                candidates[index].score *= DEMOTION;
            }
        }
        candidates.sort_by(|a, b| {
            b.confirmed
                .cmp(&a.confirmed)
                .then(b.score.total_cmp(&a.score))
        });
    }
}

fn baseband(
    iq: &[Complex<f32>],
    rate: f64,
    band: &Band,
) -> Result<Vec<Complex<f32>>, ChannelError> {
    let wanted = (SEARCH_SECONDS * rate) as usize;
    let tail = &iq[iq.len().saturating_sub(wanted)..];
    let mut ddc = Ddc::new(rate, SYNC_SEARCH_RATE_HZ, band.center_hz)
        .map_err(|e| ChannelError::InvalidSettings(e.to_string()))?;
    let mut out = Vec::with_capacity((tail.len() as f64 * SYNC_SEARCH_RATE_HZ / rate) as usize + 1);
    ddc.process(tail, &mut out);
    Ok(out)
}

fn plausible(mode: &SyncSearch, hits: u32, positions: usize, rigour: Rigour) -> bool {
    let (least, margin) = match rigour {
        Rigour::Candidate => (mode.sync.min_hits, CHANCE_MARGIN),
        Rigour::Probe => (mode.sync.min_hits.max(PROBE_MIN_HITS), PROBE_CHANCE_MARGIN),
    };
    let densest = (positions / SHORTEST_FRAME_SYMBOLS) as u32 + 1;
    hits >= least
        && hits <= densest
        && f64::from(hits) >= margin * chance_hits(mode, positions) + 1.0
}

fn chance_hits(mode: &SyncSearch, positions: usize) -> f64 {
    let bits = f64::from(mode.sync.bits);
    let mut within = 0.0;
    let mut choose = 1.0;
    for k in 0..=mode.sync.tolerance {
        if k > 0 {
            choose *= (bits - f64::from(k) + 1.0) / f64::from(k);
        }
        within += choose;
    }
    within / 2f64.powf(bits) * positions as f64 * mode.sync.patterns.len() as f64
}

/// How many of `mode`'s sync words the soft symbols hold.
pub fn hits(soft: &[f32], mode: &SyncSearch) -> u32 {
    let sync = &mode.sync;
    let mapping = mode.params.mapping();
    let bits_per_symbol = mapping.bits_per_symbol();
    let (offset, scale) = level_fit(soft, mapping);
    let mask = if sync.bits >= 64 {
        u64::MAX
    } else {
        (1u64 << sync.bits) - 1
    };
    let mut register = 0u64;
    let mut found = 0;
    for (position, &symbol) in soft.iter().enumerate() {
        register =
            register << bits_per_symbol | u64::from(mapping.slice((symbol - offset) * scale));
        if (position + 1) * (bits_per_symbol as usize) < sync.bits as usize {
            continue;
        }
        if sync
            .patterns
            .iter()
            .any(|&pattern| hamming_distance(register & mask, pattern & mask) <= sync.tolerance)
        {
            found += 1;
        }
    }
    found
}

fn level_fit(soft: &[f32], mapping: &sdrmm_modem::cpm::Mapping) -> (f32, f32) {
    let n = soft.len().max(1) as f64;
    let mean = soft.iter().map(|&s| f64::from(s)).sum::<f64>() / n;
    let measured = (soft
        .iter()
        .map(|&s| (f64::from(s) - mean) * (f64::from(s) - mean))
        .sum::<f64>()
        / n)
        .sqrt();
    let levels = mapping.levels();
    let expected = (levels
        .iter()
        .map(|&l| f64::from(l) * f64::from(l))
        .sum::<f64>()
        / levels.len() as f64)
        .sqrt();
    let scale = if measured > f64::MIN_POSITIVE {
        expected / measured
    } else {
        1.0
    };
    (mean as f32, scale as f32)
}

fn group_by_waveform<'a>(
    searchable: &[(usize, &'a SyncSearch)],
) -> Vec<Vec<(usize, &'a SyncSearch)>> {
    let mut groups: Vec<Vec<(usize, &'a SyncSearch)>> = Vec::new();
    for &entry in searchable {
        match groups.iter_mut().find(|group| {
            group
                .first()
                .is_some_and(|&(_, head): &(usize, &SyncSearch)| head.same_waveform(entry.1))
        }) {
            Some(group) => group.push(entry),
            None => groups.push(vec![entry]),
        }
    }
    groups
}
