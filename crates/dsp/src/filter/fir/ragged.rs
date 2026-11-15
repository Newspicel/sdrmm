/// Runs `process` over `input` in blocks whose lengths cycle through `sizes`, and collects
/// every block's output.
pub(crate) fn in_ragged_blocks<T, U: Clone>(
    input: &[T],
    sizes: &[usize],
    mut process: impl FnMut(&[T], &mut Vec<U>),
) -> Vec<U> {
    let mut collected = Vec::new();
    let mut block = Vec::new();
    let mut at = 0;
    for &len in sizes.iter().cycle() {
        if at >= input.len() {
            break;
        }
        let end = (at + len).min(input.len());
        process(&input[at..end], &mut block);
        collected.extend_from_slice(&block);
        at = end;
    }
    collected
}
