/// Scheduling class for a thread that must not fall behind a live radio.
///
/// macOS demotes threads it believes are doing background work and coalesces their timers, which
/// starves a capture or DSP thread long enough to lose whole USB transfers even on an idle machine.
/// Windows starts every thread at the priority of the rest of the desktop, which costs transfers
/// as soon as anything else wants the core.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Latency {
    /// Moves samples off the wire or through the DSP graph: any delay loses signal.
    Critical,
    /// Feeds a listener but tolerates a few frames of jitter.
    Interactive,
}

#[cfg(target_os = "macos")]
pub fn claim(latency: Latency) {
    let class = match latency {
        Latency::Critical => libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE,
        Latency::Interactive => libc::qos_class_t::QOS_CLASS_USER_INITIATED,
    };
    let code = unsafe { libc::pthread_set_qos_class_self_np(class, 0) };
    if code != 0 {
        tracing::warn!(
            code,
            ?latency,
            thread = std::thread::current().name().unwrap_or("unnamed"),
            "could not raise the thread's scheduling class; audio may stutter under load"
        );
    }
}

#[cfg(target_os = "windows")]
pub fn claim(latency: Latency) {
    use windows_sys::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_ABOVE_NORMAL, THREAD_PRIORITY_HIGHEST,
    };

    let priority = match latency {
        Latency::Critical => THREAD_PRIORITY_HIGHEST,
        Latency::Interactive => THREAD_PRIORITY_ABOVE_NORMAL,
    };
    // SAFETY: the pseudo handle from GetCurrentThread names this thread, needs no closing, and
    // both calls take it by value.
    if unsafe { SetThreadPriority(GetCurrentThread(), priority) } == 0 {
        tracing::warn!(
            ?latency,
            thread = std::thread::current().name().unwrap_or("unnamed"),
            "could not raise the thread's scheduling class; audio may stutter under load"
        );
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn claim(latency: Latency) {
    use rustix::{process, thread};

    let wanted = match latency {
        Latency::Critical => -10,
        Latency::Interactive => -5,
    };
    let tid = Some(thread::gettid());
    let current = process::getpriority_process(tid).unwrap_or(0);
    if current <= wanted {
        return;
    }
    let reached = raise(wanted, nice_floor(), current, |nice| {
        process::setpriority_process(tid, nice).is_ok()
    });
    if reached != Some(wanted) {
        warn_once(latency, reached);
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn raise(wanted: i32, floor: i32, current: i32, mut set: impl FnMut(i32) -> bool) -> Option<i32> {
    [wanted, wanted.max(floor)]
        .into_iter()
        .find(|&nice| nice < current && set(nice))
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn nice_floor() -> i32 {
    match rustix::process::getrlimit(rustix::process::Resource::Nice).current {
        Some(limit) => 20 - i32::try_from(limit.min(40)).unwrap_or(40),
        None => -20,
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn warn_once(latency: Latency, reached: Option<i32>) {
    use std::sync::atomic::{AtomicBool, Ordering};

    static WARNED: AtomicBool = AtomicBool::new(false);
    if !WARNED.swap(true, Ordering::Relaxed) {
        tracing::warn!(
            ?latency,
            nice = reached.unwrap_or(0),
            thread = std::thread::current().name().unwrap_or("unnamed"),
            "could not fully raise the thread's priority; allow it with RLIMIT_NICE or CAP_SYS_NICE, or audio may stutter under load"
        );
    }
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "windows",
    target_os = "linux",
    target_os = "android"
)))]
pub fn claim(_latency: Latency) {}

/// Holds off the throttling the OS applies to an app it thinks is idle.
///
/// macOS App Nap judges by the window, not by the radio: an app whose window is covered or on
/// another space gets its threads demoted and its timers coalesced by seconds, which is long
/// enough to overflow the capture ring. Audio played by the webview counts for the webview's
/// process, not this one, so listening is not enough to stay awake.
#[cfg(target_os = "macos")]
pub struct Awake(
    Option<
        objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2::runtime::NSObjectProtocol>>,
    >,
);

/// SAFETY: the token is an opaque object whose only use is being handed back to
/// `-[NSProcessInfo endActivity:]`, which Apple documents as safe to call from any thread. It is
/// never dereferenced here and never handed out.
#[cfg(target_os = "macos")]
unsafe impl Send for Awake {}

/// SAFETY: see the `Send` impl: the token is opaque and never dereferenced.
#[cfg(target_os = "macos")]
unsafe impl Sync for Awake {}

#[cfg(not(target_os = "macos"))]
pub struct Awake;

#[cfg(target_os = "macos")]
#[must_use]
pub fn stay_awake(reason: &str) -> Awake {
    use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};

    let token = NSProcessInfo::processInfo().beginActivityWithOptions_reason(
        NSActivityOptions::UserInitiated,
        &NSString::from_str(reason),
    );
    Awake(Some(token))
}

#[cfg(not(target_os = "macos"))]
#[must_use]
pub fn stay_awake(_reason: &str) -> Awake {
    Awake
}

#[cfg(target_os = "macos")]
impl Drop for Awake {
    fn drop(&mut self) {
        use objc2_foundation::NSProcessInfo;

        if let Some(token) = self.0.take() {
            unsafe { NSProcessInfo::processInfo().endActivity(&token) };
        }
    }
}

/// Keeps the token an RAII guard on platforms that have nothing to release, so callers and tests
/// can rely on the same drop semantics everywhere.
#[cfg(not(target_os = "macos"))]
impl Drop for Awake {
    fn drop(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claiming_twice_on_one_thread_is_accepted() {
        claim(Latency::Critical);
        claim(Latency::Interactive);
        claim(Latency::Critical);
    }

    #[test]
    fn an_activity_can_be_held_and_released_from_another_thread() {
        let awake = stay_awake("test");
        std::thread::spawn(move || drop(awake))
            .join()
            .expect("ending the activity off-thread panicked");
    }

    #[test]
    fn overlapping_activities_release_independently() {
        let outer = stay_awake("outer");
        let inner = stay_awake("inner");
        drop(inner);
        drop(outer);
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn raising_takes_the_wanted_nice_then_the_allowed_floor_then_gives_up() {
        assert_eq!(raise(-10, -20, 0, |_| true), Some(-10));
        assert_eq!(raise(-10, -4, 0, |nice| nice == -4), Some(-4));
        assert_eq!(raise(-10, 20, 0, |nice| nice == -10), Some(-10));
        assert_eq!(raise(-10, 20, 0, |nice| nice >= 0), None);
        assert_eq!(raise(-10, -4, -6, |nice| nice == -4), None);
        assert_eq!(raise(-10, 0, 0, |_| false), None);
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn a_claimed_thread_never_ends_below_where_it_started() {
        use rustix::{process, thread};

        std::thread::spawn(|| {
            let tid = Some(thread::gettid());
            let before = process::getpriority_process(tid).expect("getpriority failed");
            claim(Latency::Critical);
            let after = process::getpriority_process(tid).expect("getpriority failed");
            assert!(after <= before, "nice went from {before} to {after}");
            if nice_floor() <= -10 {
                assert_eq!(after, -10);
            }
        })
        .join()
        .expect("thread panicked");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_claimed_thread_reports_the_class_it_asked_for() {
        std::thread::spawn(|| {
            claim(Latency::Critical);
            let mut class = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
            let code = unsafe {
                libc::pthread_get_qos_class_np(
                    libc::pthread_self(),
                    &raw mut class,
                    std::ptr::null_mut(),
                )
            };
            assert_eq!(code, 0, "pthread_get_qos_class_np failed");
            assert_eq!(
                class as u32,
                libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE as u32
            );
        })
        .join()
        .expect("thread panicked");
    }
}
