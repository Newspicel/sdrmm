use super::gf::SmallField;

const FIELD: SmallField = SmallField::GF64;
const GENERATOR: u8 = 2;
const MAX_PARITY: usize = 32;

#[must_use]
pub fn rs64_encode(data: &[u8], parity_symbols: usize) -> Vec<u8> {
    let generator = generator_poly(parity_symbols);
    let mut codeword: Vec<u8> = data.iter().map(|symbol| symbol & FIELD.mask()).collect();
    codeword.resize(data.len() + parity_symbols, 0);
    for index in 0..data.len() {
        let coefficient = codeword[index];
        if coefficient == 0 {
            continue;
        }
        for (offset, &factor) in generator.iter().enumerate() {
            codeword[index + offset] ^= FIELD.mul(factor, coefficient);
        }
    }
    codeword[..data.len()].copy_from_slice(data);
    codeword
}

#[must_use]
pub fn rs64_decode(codeword: &[u8], data_symbols: usize) -> Option<(Vec<u8>, u32)> {
    let parity = codeword.len().checked_sub(data_symbols)?;
    if parity == 0 || parity >= MAX_PARITY || codeword.len() > FIELD.order() as usize {
        return None;
    }
    let mut corrected: Vec<u8> = codeword
        .iter()
        .map(|symbol| symbol & FIELD.mask())
        .collect();
    let syndromes = syndromes(&corrected, parity);
    if syndromes.iter().all(|&value| value == 0) {
        return Some((corrected[..data_symbols].to_vec(), 0));
    }
    let (locator, degree) = berlekamp_massey(&syndromes)?;
    if degree == 0 || degree > parity / 2 {
        return None;
    }
    let (positions, powers) = error_positions(&locator[..=degree], corrected.len());
    if positions.len() != degree {
        return None;
    }
    let values = error_values(&powers, &syndromes)?;
    for (&position, value) in positions.iter().zip(values) {
        corrected[position] ^= value;
    }
    if syndromes_of(&corrected, parity).any(|value| value != 0) {
        return None;
    }
    Some((corrected[..data_symbols].to_vec(), degree as u32))
}

fn generator_poly(parity_symbols: usize) -> Vec<u8> {
    let mut generator = vec![1u8];
    for root in 1..=parity_symbols {
        let mut next = vec![0u8; generator.len() + 1];
        for (index, &coefficient) in generator.iter().enumerate() {
            next[index] ^= coefficient;
            next[index + 1] ^= FIELD.mul(coefficient, FIELD.pow(GENERATOR, root as i32));
        }
        generator = next;
    }
    generator
}

fn syndromes(codeword: &[u8], count: usize) -> Vec<u8> {
    syndromes_of(codeword, count).collect()
}

fn syndromes_of(codeword: &[u8], count: usize) -> impl Iterator<Item = u8> + '_ {
    (1..=count).map(|root| {
        let x = FIELD.pow(GENERATOR, root as i32);
        codeword
            .iter()
            .fold(0, |value, &symbol| FIELD.mul(value, x) ^ symbol)
    })
}

/// The error locator in ascending powers and its degree.
fn berlekamp_massey(syndromes: &[u8]) -> Option<(Vec<u8>, usize)> {
    let parity = syndromes.len();
    let mut locator = vec![0u8; parity + 1];
    let mut previous = vec![0u8; parity + 1];
    locator[0] = 1;
    previous[0] = 1;
    let (mut degree, mut shift, mut discrepancy_at_update) = (0usize, 1usize, 1u8);
    for n in 0..parity {
        let mut discrepancy = syndromes[n];
        for i in 1..=degree {
            discrepancy ^= FIELD.mul(locator[i], syndromes[n - i]);
        }
        if discrepancy == 0 {
            shift += 1;
            continue;
        }
        let saved = locator.clone();
        let scale = FIELD.mul(discrepancy, FIELD.inv(discrepancy_at_update)?);
        for i in 0..=parity - shift {
            locator[i + shift] ^= FIELD.mul(scale, previous[i]);
        }
        if 2 * degree <= n {
            degree = n + 1 - degree;
            previous = saved;
            discrepancy_at_update = discrepancy;
            shift = 1;
        } else {
            shift += 1;
        }
    }
    Some((locator, degree))
}

/// Chien search: the positions whose inverse power is a root of the locator, with those powers.
fn error_positions(locator: &[u8], len: usize) -> (Vec<usize>, Vec<usize>) {
    (0..len)
        .filter_map(|position| {
            let power = len - 1 - position;
            let root = FIELD.pow(GENERATOR, -(power as i32));
            (FIELD.eval_ascending(locator, root) == 0).then_some((position, power))
        })
        .unzip()
}

/// Solves the syndrome equations for the error values by Gauss-Jordan elimination.
fn error_values(powers: &[usize], syndromes: &[u8]) -> Option<Vec<u8>> {
    let degree = powers.len();
    let mut matrix = vec![vec![0u8; degree + 1]; degree];
    for (row, equation) in matrix.iter_mut().enumerate() {
        for (column, &power) in powers.iter().enumerate() {
            equation[column] = FIELD.pow(GENERATOR, ((row + 1) * power) as i32);
        }
        equation[degree] = syndromes[row];
    }
    for column in 0..degree {
        let pivot = (column..degree).find(|&row| matrix[row][column] != 0)?;
        matrix.swap(column, pivot);
        let inverse = FIELD.inv(matrix[column][column])?;
        for value in &mut matrix[column][column..=degree] {
            *value = FIELD.mul(*value, inverse);
        }
        let pivot_row = matrix[column][column..=degree].to_vec();
        for (row, target_row) in matrix.iter_mut().enumerate() {
            if row == column {
                continue;
            }
            let scale = target_row[column];
            for (value, &pivot_value) in target_row[column..=degree].iter_mut().zip(&pivot_row) {
                *value ^= FIELD.mul(scale, pivot_value);
            }
        }
    }
    Some(matrix.iter().map(|row| row[degree]).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn p25_reed_solomon_corrects_to_each_codes_capacity() {
        for (data_symbols, parity_symbols) in [(12, 12), (16, 8), (20, 16)] {
            let data: Vec<u8> = (0..data_symbols)
                .map(|index| (index * 7 + 3) as u8 & 0x3F)
                .collect();
            let clean = rs64_encode(&data, parity_symbols);
            assert_eq!(rs64_decode(&clean, data_symbols), Some((data.clone(), 0)));
            let mut damaged = clean;
            for index in 0..parity_symbols / 2 {
                damaged[index * 2] ^= (index as u8 + 1) & 0x3F;
            }
            assert_eq!(
                rs64_decode(&damaged, data_symbols),
                Some((data, (parity_symbols / 2) as u32))
            );
        }
    }
}
