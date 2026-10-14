use std::sync::atomic::{AtomicBool, Ordering};

use crate::dongle::Error;

pub(crate) fn apply_pausing<G>(
    pause_first: &AtomicBool,
    mut apply: impl FnMut() -> Result<(), Error>,
    pause: impl FnOnce() -> Result<G, Error>,
) -> Result<(), Error> {
    if !pause_first.load(Ordering::Acquire) {
        match apply() {
            Err(error) if error.is_stall() => {
                tracing::warn!(%error, "RTL-SDR stalls on control while streaming, settings now pause the stream");
                pause_first.store(true, Ordering::Release);
            }
            outcome => return outcome,
        }
    }
    let _held = pause()?;
    apply()
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use nusb::transfer::TransferError;
    use sdrmm_usb_stream::StreamError;

    use super::*;

    struct Held<'a>(&'a Cell<bool>);

    impl Drop for Held<'_> {
        fn drop(&mut self) {
            self.0.set(false);
        }
    }

    fn stall() -> Error {
        Error::Control {
            access: crate::dongle::Access::Demod {
                page: 1,
                reg: 0x01,
                dir: crate::dongle::Dir::Write,
            },
            source: TransferError::Stall,
        }
    }

    fn run(
        pause_first: &AtomicBool,
        outcomes: Vec<Result<(), Error>>,
    ) -> (Result<(), Error>, Vec<bool>, usize) {
        let holding = Cell::new(false);
        let pauses = Cell::new(0);
        let seen = RefCell::new(Vec::new());
        let mut outcomes = outcomes.into_iter();
        let result = apply_pausing(
            pause_first,
            || {
                seen.borrow_mut().push(holding.get());
                outcomes.next().unwrap_or(Ok(()))
            },
            || {
                pauses.set(pauses.get() + 1);
                holding.set(true);
                Ok(Held(&holding))
            },
        );
        assert!(!holding.get(), "the stream resumes afterwards");
        (result, seen.into_inner(), pauses.get())
    }

    #[test]
    fn a_radio_that_takes_control_keeps_streaming() {
        let flag = AtomicBool::new(false);
        let (result, seen, pauses) = run(&flag, vec![Ok(())]);
        assert!(result.is_ok());
        assert_eq!((seen, pauses), (vec![false], 0));
        assert!(!flag.load(Ordering::Acquire));
    }

    #[test]
    fn a_stall_retries_with_the_stream_paused_and_remembers() {
        let flag = AtomicBool::new(false);
        let (result, seen, pauses) = run(&flag, vec![Err(stall()), Ok(())]);
        assert!(result.is_ok());
        assert_eq!((seen, pauses), (vec![false, true], 1));
        assert!(flag.load(Ordering::Acquire));
        let (result, seen, pauses) = run(&flag, vec![Ok(())]);
        assert!(result.is_ok());
        assert_eq!((seen, pauses), (vec![true], 1));
    }

    #[test]
    fn other_errors_do_not_pause() {
        let flag = AtomicBool::new(false);
        let (result, seen, pauses) = run(&flag, vec![Err(Error::NoTuner)]);
        assert!(matches!(result, Err(Error::NoTuner)));
        assert_eq!((seen, pauses), (vec![false], 0));
    }

    #[test]
    fn a_stall_while_paused_surfaces() {
        let flag = AtomicBool::new(false);
        let (result, _, _) = run(&flag, vec![Err(stall()), Err(stall())]);
        assert!(result.is_err_and(|error| error.is_stall()));
    }

    #[test]
    fn a_stream_that_will_not_pause_surfaces() {
        let flag = AtomicBool::new(true);
        let result = apply_pausing(
            &flag,
            || panic!("nothing is written without the pause"),
            || Err::<(), _>(Error::Stream(StreamError::Pause("the stream ended"))),
        );
        assert!(matches!(result, Err(Error::Stream(StreamError::Pause(_)))));
    }
}
