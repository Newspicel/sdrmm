/// Keeps the `capacity` items with the largest key, counting every item it had to drop.
pub(crate) struct Strongest {
    capacity: usize,
    heaped: bool,
    dropped: usize,
}

impl Strongest {
    pub(crate) const fn new(capacity: usize) -> Self {
        Self {
            capacity,
            heaped: false,
            dropped: 0,
        }
    }

    pub(crate) fn offer<T: Copy>(&mut self, out: &mut Vec<T>, item: T, key: impl Fn(&T) -> f32) {
        if out.len() < self.capacity {
            out.push(item);
            return;
        }
        self.dropped += 1;
        if out.is_empty() {
            return;
        }
        if !self.heaped {
            for index in (0..out.len() / 2).rev() {
                sift_down(out, index, &key);
            }
            self.heaped = true;
        }
        if key(&item) > key(&out[0]) {
            out[0] = item;
            sift_down(out, 0, &key);
        }
    }

    pub(crate) const fn dropped(&self) -> usize {
        self.dropped
    }
}

fn sift_down<T>(heap: &mut [T], mut index: usize, key: &impl Fn(&T) -> f32) {
    loop {
        let left = 2 * index + 1;
        let right = left + 1;
        let mut smallest = index;
        if left < heap.len() && key(&heap[left]) < key(&heap[smallest]) {
            smallest = left;
        }
        if right < heap.len() && key(&heap[right]) < key(&heap[smallest]) {
            smallest = right;
        }
        if smallest == index {
            return;
        }
        heap.swap(index, smallest);
        index = smallest;
    }
}
