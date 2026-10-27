use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock, Weak},
};

use sdrmm_device::{DeviceError, lock};

pub(crate) trait Pins: Send + Sync {
    fn drive(&self, pin: u8, high: bool) -> Result<(), DeviceError>;
}

type Held = Mutex<HashMap<String, Weak<dyn Pins>>>;

fn held() -> &'static Held {
    static HELD: OnceLock<Held> = OnceLock::new();
    HELD.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn hold_control(id: &str, pins: &Arc<dyn Pins>) {
    let mut banks = lock(held());
    banks.retain(|_, held| held.strong_count() > 0);
    banks.insert(id.to_owned(), Arc::downgrade(pins));
}

pub(crate) fn control(id: &str) -> Option<Arc<dyn Pins>> {
    lock(held()).get(id).and_then(Weak::upgrade)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    #[derive(Default)]
    struct Counter(AtomicU32);

    impl Pins for Counter {
        fn drive(&self, pin: u8, _high: bool) -> Result<(), DeviceError> {
            self.0.fetch_add(u32::from(pin), Ordering::Release);
            Ok(())
        }
    }

    #[test]
    fn a_bank_reaches_its_control_lane_while_that_lane_is_open() {
        assert!(control("bench/1").is_none());
        let counter = Arc::new(Counter::default());
        let pins: Arc<dyn Pins> = counter.clone();
        hold_control("bench/1", &pins);
        let found = control("bench/1").expect("the lane that holds the pins");
        found.drive(3, true).expect("a pin moves");
        assert_eq!(counter.0.load(Ordering::Acquire), 3);
        drop(found);
        drop(pins);
        drop(counter);
        assert!(control("bench/1").is_none());
    }
}
