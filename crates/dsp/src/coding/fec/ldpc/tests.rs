use super::*;

const SIZE: usize = 12;
const BLOCKS: usize = 6;

fn circulant(block: usize, shift: usize) -> Circulant {
    Circulant {
        base: block * SIZE,
        shift,
        open: false,
    }
}

fn layout() -> QcLayout {
    let layers = [
        [(0, 1), (1, 5), (2, 0), (3, 7), (4, 2)],
        [(0, 3), (1, 0), (2, 9), (4, 6), (5, 4)],
        [(1, 2), (2, 4), (3, 11), (4, 1), (5, 8)],
    ]
    .iter()
    .map(|layer| layer.iter().map(|&(b, s)| circulant(b, s)).collect())
    .collect();
    QcLayout::new(SIZE, BLOCKS * SIZE, layers).expect("a layout")
}

fn next(state: &mut u32) -> u32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    *state
}

fn null_space(code: &ParityCheck) -> Vec<Vec<bool>> {
    let n = code.variables();
    let mut rows: Vec<Vec<bool>> = (0..code.checks())
        .map(|check| {
            let mut row = vec![false; n];
            for &variable in code.check(check) {
                row[variable] ^= true;
            }
            row
        })
        .collect();
    let mut pivots = Vec::new();
    let mut rank = 0;
    for column in 0..n {
        let Some(found) = (rank..rows.len()).find(|&row| rows[row][column]) else {
            continue;
        };
        rows.swap(rank, found);
        for row in 0..rows.len() {
            if row != rank && rows[row][column] {
                let pivot = rows[rank].clone();
                for (bit, &other) in rows[row].iter_mut().zip(&pivot) {
                    *bit ^= other;
                }
            }
        }
        pivots.push(column);
        rank += 1;
    }
    (0..n)
        .filter(|column| !pivots.contains(column))
        .map(|free| {
            let mut word = vec![false; n];
            word[free] = true;
            for (row, &pivot) in pivots.iter().enumerate() {
                word[pivot] = rows[row][free];
            }
            word
        })
        .collect()
}

fn codeword(code: &ParityCheck, seed: u32) -> Vec<bool> {
    let mut state = seed | 1;
    let mut word = vec![false; code.variables()];
    for basis in null_space(code) {
        if next(&mut state) & 1 == 1 {
            for (bit, &other) in word.iter_mut().zip(&basis) {
                *bit ^= other;
            }
        }
    }
    word
}

fn llrs(word: &[bool]) -> Vec<f32> {
    word.iter()
        .map(|&bit| if bit { -4.0 } else { 4.0 })
        .collect()
}

fn damaged(word: &[bool], at: usize) -> Vec<f32> {
    let mut received = llrs(word);
    received[at] = -received[at] / 4.0;
    received
}

#[test]
fn the_matrix_grows_to_hold_every_pair() {
    let code = ParityCheck::from_edges(2, 3, &[(2, 0), (0, 4)]);
    assert_eq!((code.checks(), code.variables()), (3, 5));
    assert!(code.variable_edges(3).is_empty());
}

#[test]
fn a_hamming_code_checks_its_words() {
    let edges = [
        (0, 0),
        (0, 1),
        (0, 2),
        (0, 4),
        (1, 1),
        (1, 2),
        (1, 3),
        (1, 5),
        (2, 0),
        (2, 1),
        (2, 3),
        (2, 6),
    ];
    let code = ParityCheck::from_edges(3, 7, &edges);
    assert_eq!((code.checks(), code.variables(), code.edges()), (3, 7, 12));
    assert_eq!(code.check(1), &[1, 2, 3, 5]);
    assert_eq!(code.variable_edges(1).len(), 3);
    let word = [true, false, false, false, true, false, true];
    assert!(code.satisfies(&word));
    let mut broken = word;
    broken[1] ^= true;
    assert_eq!(code.unsatisfied(&broken), 3);
    assert_eq!(code.pairs().count(), 12);
}

#[test]
fn a_circulant_layout_expands_to_its_pairs() {
    let layout = layout();
    let code = layout.parity_check();
    assert_eq!(code.checks(), 3 * SIZE);
    assert_eq!(code.edges(), 15 * SIZE);
    assert_eq!(code.check(0), &[1, 17, 24, 43, 50]);
    let open = Circulant {
        base: 0,
        shift: SIZE - 1,
        open: true,
    };
    assert_eq!(open.variable(0, SIZE), None);
    assert_eq!(open.variable(1, SIZE), Some(0));
    assert!(QcLayout::new(SIZE, SIZE, vec![vec![circulant(1, 0)]]).is_none());
    assert!(QcLayout::new(SIZE, SIZE, vec![vec![circulant(0, SIZE)]]).is_none());
}

#[test]
fn both_schedules_repair_a_weak_error() {
    let code = layout().parity_check();
    let word = codeword(&code, 7);
    assert!(code.satisfies(&word));
    assert!(word.iter().any(|&bit| bit));
    for schedule in [Schedule::Layered, Schedule::Flooding] {
        let mut decoder = MinSum::new(code.clone(), schedule, 0.8);
        assert_eq!(decoder.decode(&llrs(&word), 10), Some(0));
        for at in [0, 23, 71] {
            let iterations = decoder.decode(&damaged(&word, at), 10);
            assert!(iterations.is_some_and(|count| count > 0), "{schedule:?}");
            assert_eq!(decoder.hard(), word.as_slice(), "{schedule:?} {at}");
        }
    }
}

#[test]
fn iterating_by_hand_matches_decode() {
    let code = layout().parity_check();
    let word = codeword(&code, 9);
    let received = damaged(&word, 30);
    let mut driven = MinSum::new(code.clone(), Schedule::Layered, 0.8);
    assert!(driven.load(&received) > 0);
    while driven.iterate(&received) > 0 {}
    let mut automatic = MinSum::new(code, Schedule::Layered, 0.8);
    assert!(automatic.decode(&received, 10).is_some());
    assert_eq!(driven.totals(), automatic.totals());
}

#[test]
fn quasi_cyclic_decoding_repairs_a_weak_error() {
    let layout = layout();
    let code = layout.parity_check();
    let word = codeword(&code, 11);
    let mut decoder = QuasiCyclic::new(layout);
    let mut hard = vec![false; word.len()];
    decoder.load(&llrs(&word), |bit| bit);
    assert_eq!(decoder.run(10), Some(0));
    for at in [5, 40, 66] {
        decoder.load(&damaged(&word, at), |bit| bit);
        assert!(decoder.run(10).is_some_and(|count| count > 0));
        decoder.harden(&mut hard);
        assert_eq!(hard, word, "{at}");
    }
}

#[test]
fn noise_does_not_converge() {
    let mut decoder = QuasiCyclic::new(layout());
    let mut state = 0x1357_9bdf;
    let noise: Vec<f32> = (0..BLOCKS * SIZE)
        .map(|_| (next(&mut state) >> 16) as f32 / 16_384.0 - 2.0)
        .collect();
    decoder.load(&noise, |bit| bit);
    assert!(decoder.run(20).is_none());
}

fn kernels() -> Vec<Kernel> {
    let mut kernels = vec![Kernel::Scalar, Kernel::Vector];
    if !kernels.contains(&Kernel::detect()) {
        kernels.push(Kernel::detect());
    }
    kernels
}

#[test]
fn every_kernel_updates_a_layer_bit_exactly() {
    let mut state = 0x51d;
    for (degree, stride) in [(3, 16), (7, 368), (11, 368), (30, 48)] {
        let rows = degree * stride;
        let mut sample = || match next(&mut state) % 16 {
            0 => i16::MIN,
            1 => i16::MAX,
            2 => 0,
            3 => -1,
            _ => next(&mut state) as i16,
        };
        let gathered: Vec<i16> = (0..rows).map(|_| sample()).collect();
        let messages: Vec<i16> = (0..rows).map(|_| sample()).collect();
        let outcomes: Vec<_> = kernels()
            .into_iter()
            .map(|kernel| {
                let mut gathered = gathered.clone();
                let mut extrinsic = vec![0; rows];
                let mut messages = messages.clone();
                let unsatisfied =
                    kernel.update(stride, &mut gathered, &mut extrinsic, &mut messages);
                (unsatisfied, gathered, extrinsic, messages)
            })
            .collect();
        for outcome in &outcomes[1..] {
            assert!(*outcome == outcomes[0], "degree {degree}");
        }
    }
}
