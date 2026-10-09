#[cfg(target_os = "linux")]
pub fn pin(cpu: usize) {
    let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    if cores < 2 {
        return;
    }
    let mut set: libc::cpu_set_t = unsafe { std::mem::zeroed() };
    unsafe { libc::CPU_SET(cpu % cores, &mut set) };
    let pinned =
        unsafe { libc::sched_setaffinity(0, size_of::<libc::cpu_set_t>(), &raw const set) };
    if pinned != 0 {
        eprintln!(
            "iqlinkd: could not pin to cpu {cpu}: {}",
            std::io::Error::last_os_error()
        );
    }
}

#[cfg(not(target_os = "linux"))]
pub const fn pin(_cpu: usize) {}
