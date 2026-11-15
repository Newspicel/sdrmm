/// Fills `branch` with branch `phase` of `proto` split `phases` ways, time-reversed and scaled
/// to unity DC gain.
pub(crate) fn reversed_branch(proto: &[f32], phases: usize, phase: usize, branch: &mut [f32]) {
    for (k, slot) in branch.iter_mut().rev().enumerate() {
        *slot = proto[k * phases + phase];
    }
    let sum: f32 = branch.iter().rev().sum();
    debug_assert!(sum > 0.0, "degenerate polyphase branch");
    for value in branch.iter_mut() {
        *value /= sum;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_branch_takes_every_nth_tap_backwards_at_unity_gain() {
        let proto = [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];
        let mut branch = [0.0f32; 3];
        reversed_branch(&proto, 3, 1, &mut branch);
        assert_eq!(branch, [8.0 / 15.0, 5.0 / 15.0, 2.0 / 15.0]);
    }
}
