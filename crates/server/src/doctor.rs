use std::path::Path;

use sdrmm_wire::{
    ArrayStatus, CheckStatus, DeviceBackend, DeviceInfo, DoctorCheck, DoctorReport, SyncState,
};

const LANE_SUFFIX: &str = " lane ";

#[must_use]
pub fn collect(db_path: Option<&Path>, recordings_dir: Option<&Path>) -> DoctorReport {
    let registry = sdrmm_engine::builtin_registry(None);
    report(&registry, db_path, recordings_dir)
}

#[must_use]
pub fn report(
    registry: &sdrmm_device::DeviceRegistry,
    db_path: Option<&Path>,
    recordings_dir: Option<&Path>,
) -> DoctorReport {
    let mut checks = vec![backends_check(registry)];
    checks.extend(registry.health());
    checks.extend(devices_checks(registry));
    checks.extend(usb_checks());
    checks.push(path_check(
        "storage.db",
        "Database",
        db_path,
        PathKind::File,
        "presets, bookmarks, recordings index and decoder log are kept in memory and lost on \
         exit",
    ));
    checks.push(path_check(
        "storage.recordings",
        "Recordings directory",
        recordings_dir,
        PathKind::Directory,
        "recording is disabled",
    ));
    DoctorReport {
        version: env!("CARGO_PKG_VERSION").to_string(),
        platform: format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH),
        checks,
    }
}

fn backends_check(registry: &sdrmm_device::DeviceRegistry) -> DoctorCheck {
    let mut backends: Vec<DeviceBackend> = registry
        .driver_ids()
        .into_iter()
        .map(|(_, backend)| backend)
        .collect();
    backends.sort_unstable_by_key(|backend| backend.as_str());
    let hardware: Vec<DeviceBackend> = backends
        .iter()
        .copied()
        .filter(|backend| backend.is_radio())
        .collect();
    let names: Vec<&str> = backends
        .iter()
        .copied()
        .map(DeviceBackend::as_str)
        .collect();
    let detail = format!("compiled backends: {}", names.join(", "));
    if hardware.is_empty() {
        let virtual_capabilities = virtual_capabilities(sdrmm_engine::synthetic_devices_offered());
        return DoctorCheck {
            id: "backends".to_string(),
            name: "Device backends".to_string(),
            status: CheckStatus::Warn,
            detail,
            hint: Some(format!(
                "this build has no hardware backend, only {virtual_capabilities}. Use a normal \
                 build, or rebuild with --features soapy."
            )),
        };
    }
    DoctorCheck {
        id: "backends".to_string(),
        name: "Device backends".to_string(),
        status: CheckStatus::Ok,
        detail,
        hint: None,
    }
}

fn virtual_capabilities(debug_build: bool) -> &'static str {
    if debug_build {
        "synthetic signal-generator and marker radios, plus SigMF playback through the virtual \
         driver"
    } else {
        "SigMF playback through the virtual driver"
    }
}

fn devices_checks(registry: &sdrmm_device::DeviceRegistry) -> Vec<DoctorCheck> {
    let timings = registry.probe_timings();
    let devices = registry.probe_all_deep();
    let mut checks = vec![devices_check(&devices, &timings)];
    checks.extend(port_bound_check(&devices));
    checks.extend(banks_check(&devices));
    checks
}

struct Bank {
    id: String,
    name: String,
    seen: u32,
    lanes: u32,
}

impl Bank {
    fn line(&self) -> String {
        format!("{}: {} of {} lanes", self.name, self.seen, self.lanes)
    }

    fn whole(&self) -> bool {
        self.seen == self.lanes
    }
}

fn banked(devices: &[DeviceInfo]) -> Vec<Bank> {
    let mut found: Vec<Bank> = Vec::new();
    for device in devices {
        let Some(bank) = device
            .profile
            .as_ref()
            .and_then(|profile| profile.bank.as_ref())
        else {
            continue;
        };
        let name = device
            .label
            .split_once(LANE_SUFFIX)
            .map_or(device.label.as_str(), |(name, _)| name)
            .to_owned();
        match found.iter_mut().find(|held| held.id == bank.id) {
            Some(held) => held.seen += 1,
            None => found.push(Bank {
                id: bank.id.clone(),
                name,
                seen: 1,
                lanes: bank.lanes,
            }),
        }
    }
    found
}

fn banks_check(devices: &[DeviceInfo]) -> Option<DoctorCheck> {
    let banks = banked(devices);
    if banks.is_empty() {
        return None;
    }
    let broken = !banks.iter().all(Bank::whole);
    Some(DoctorCheck {
        id: "devices.banks".to_string(),
        name: "Coherent banks".to_string(),
        status: if broken {
            CheckStatus::Warn
        } else {
            CheckStatus::Ok
        },
        detail: banks.iter().map(Bank::line).collect::<Vec<_>>().join("\n"),
        hint: broken.then(|| "replug the missing dongles, then power the bank again".to_string()),
    })
}

fn array_line(status: &ArrayStatus) -> String {
    let line = format!(
        "array {}: {} {}",
        status.node,
        status.sync.label(),
        status.cal.label()
    );
    match &status.failure {
        Some(failure) => format!("{line} ({failure})"),
        None => line,
    }
}

#[must_use]
pub(crate) fn arrays_check(statuses: &[ArrayStatus]) -> Option<DoctorCheck> {
    if statuses.is_empty() {
        return None;
    }
    let steady = statuses
        .iter()
        .all(|status| status.sync == SyncState::Locked && status.failure.is_none());
    Some(DoctorCheck {
        id: "arrays".to_string(),
        name: "Arrays".to_string(),
        status: if steady {
            CheckStatus::Ok
        } else {
            CheckStatus::Warn
        },
        detail: statuses
            .iter()
            .map(array_line)
            .collect::<Vec<_>>()
            .join("\n"),
        hint: None,
    })
}

#[must_use]
pub(crate) fn served(engine: &sdrmm_engine::Engine, db_path: Option<&Path>) -> DoctorReport {
    let mut report = report(engine.registry(), db_path, engine.recordings_dir());
    report.checks.extend(arrays_check(&engine.array_statuses()));
    report
}

fn port_bound_check(devices: &[DeviceInfo]) -> Option<DoctorCheck> {
    let bound: Vec<&str> = devices
        .iter()
        .filter(|device| device.driver == DeviceBackend::Rtlsdr && device.serial.is_none())
        .map(|device| device.label.as_str())
        .collect();
    if bound.is_empty() {
        return None;
    }
    Some(DoctorCheck {
        id: "devices.serials".to_string(),
        name: "RTL-SDR serials".to_string(),
        status: CheckStatus::Warn,
        detail: format!(
            "known by USB port, not serial, so settings may follow the wrong dongle:\n{}",
            bound.join("\n")
        ),
        hint: Some(
            "give each dongle its own serial: plug in one at a time and run \
             `rtl_eeprom -s <serial>`, then replug it"
                .to_string(),
        ),
    })
}

fn devices_check(
    devices: &[DeviceInfo],
    timings: &[(DeviceBackend, usize, std::time::Duration)],
) -> DoctorCheck {
    let mut hardware: Vec<String> = devices
        .iter()
        .filter(|d| d.driver.is_radio())
        .map(|d| match &d.serial {
            Some(serial) => format!("{} [{}] serial {serial}", d.id(), d.label),
            None => format!("{} [{}]", d.id(), d.label),
        })
        .collect();
    let searches: Vec<String> = timings
        .iter()
        .map(|(driver, found, took)| format!("{driver}: {found} found in {} ms", took.as_millis()))
        .collect();
    if hardware.is_empty() {
        return DoctorCheck {
            id: "devices".to_string(),
            name: "Devices found".to_string(),
            status: CheckStatus::Warn,
            detail: format!(
                "no hardware; {} virtual device(s)\n{}",
                devices.len(),
                searches.join("\n")
            ),
            hint: Some(
                "check the USB connection, then the USB permission line below; \
                 `lsusb`/`system_profiler SPUSBDataType` shows whether the OS sees it at all"
                    .to_string(),
            ),
        };
    }
    hardware.extend(searches);
    DoctorCheck {
        id: "devices".to_string(),
        name: "Devices found".to_string(),
        status: CheckStatus::Ok,
        detail: hardware.join("\n"),
        hint: None,
    }
}

#[must_use]
pub fn rate_report(registry: &sdrmm_device::DeviceRegistry) -> DoctorReport {
    let checks = registry
        .probe_all_deep()
        .into_iter()
        .filter(|d| d.driver.is_radio())
        .map(|info| match registry.open(&info.id()) {
            Ok((_, mut device)) => {
                let rates = device.capabilities().sample_rates.clone();
                let ranges = device.capabilities().sample_rate_ranges.clone();
                let restore = device.settings().sample_rate;
                let held: Vec<(f64, Option<f64>)> = rates
                    .iter()
                    .map(|&rate| (rate, hold_rate(device.as_mut(), rate)))
                    .collect();
                if let Some(rate) = restore {
                    let _ = hold_rate(device.as_mut(), rate);
                }
                rate_check(&info.id(), &info.label, &held, &ranges)
            }
            Err(error) => DoctorCheck {
                id: format!("rates.{}", info.id()),
                name: format!("Sample rates: {}", info.label),
                status: CheckStatus::Warn,
                detail: format!("could not open: {error}"),
                hint: Some("stop anything else using the radio, then run this again".to_string()),
            },
        })
        .collect();
    DoctorReport {
        version: env!("CARGO_PKG_VERSION").to_string(),
        platform: format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH),
        checks,
    }
}

fn hold_rate(device: &mut dyn sdrmm_device::SdrDevice, rate: f64) -> Option<f64> {
    let request = sdrmm_wire::DeviceSettings {
        sample_rate: Some(rate),
        ..sdrmm_wire::DeviceSettings::default()
    };
    device.apply(&request).ok()?;
    device.settings().sample_rate
}

const RATE_TOLERANCE: f64 = 1e-6;

/// A radio that resamples across a window has no fixed rate to hold, so nothing can fail here.
/// Only a radio that declares neither discrete rates nor a range is unverifiable.
fn rate_check(
    id: &str,
    label: &str,
    held: &[(f64, Option<f64>)],
    ranges: &[sdrmm_wire::Range],
) -> DoctorCheck {
    if held.is_empty() {
        let (status, detail) = if ranges.is_empty() {
            (
                CheckStatus::Warn,
                "the driver advertises neither discrete rates nor a range to check".to_string(),
            )
        } else {
            let windows: Vec<String> = ranges
                .iter()
                .map(|range| format!("{:.0}-{:.0} Hz", range.min, range.max))
                .collect();
            (
                CheckStatus::Ok,
                format!("continuous across {}", windows.join(", ")),
            )
        };
        return DoctorCheck {
            id: format!("rates.{id}"),
            name: format!("Sample rates: {label}"),
            status,
            detail,
            hint: None,
        };
    }
    let mut lines = Vec::with_capacity(held.len());
    let mut bad = 0usize;
    for &(asked, got) in held {
        match got {
            Some(got) if ((got - asked) / asked).abs() <= RATE_TOLERANCE => {
                lines.push(format!("{:>12.0} Hz  ok", asked));
            }
            Some(got) => {
                bad += 1;
                lines.push(format!("{asked:>12.0} Hz  HELD {got:.0} Hz"));
            }
            None => {
                bad += 1;
                lines.push(format!("{asked:>12.0} Hz  refused"));
            }
        }
    }
    DoctorCheck {
        id: format!("rates.{id}"),
        name: format!("Sample rates: {label}"),
        status: if bad == 0 {
            CheckStatus::Ok
        } else {
            CheckStatus::Fail
        },
        detail: lines.join("\n"),
        hint: (bad > 0).then(|| {
            "this radio does not run at every rate it advertises; the rates marked above are \
             the ones to avoid"
                .to_string()
        }),
    }
}

fn usb_checks() -> Vec<DoctorCheck> {
    if !cfg!(target_os = "linux") {
        return Vec::new();
    }
    vec![usb_permission_check(&sdrmm_device::usb::radio_nodes())]
}

fn current_user() -> u32 {
    #[cfg(target_os = "linux")]
    {
        unsafe { libc::geteuid() }
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}

fn in_container() -> bool {
    Path::new("/.dockerenv").exists() || Path::new("/run/.containerenv").exists()
}

fn node_line(node: &sdrmm_device::usb::RadioNode, user: u32) -> String {
    let path = node.path.display();
    if !node.present {
        return format!("{}: {path} is not visible to this process", node.name);
    }
    let owner = format!("owner {}:{}, mode {:04o}", node.uid, node.gid, node.mode);
    if node.openable {
        format!("{}: {path} can be opened ({owner})", node.name)
    } else {
        format!(
            "{}: {path} cannot be opened by uid {user} ({owner})",
            node.name
        )
    }
}

fn permission_hint(blocked: &[&sdrmm_device::usb::RadioNode]) -> String {
    let container = in_container();
    if blocked.iter().any(|node| !node.present) {
        return if container {
            "the USB bus is not in this container: pass `--device /dev/bus/usb:/dev/bus/usb`, \
             and the cgroup rule `c 189:* rmw` so a replugged radio stays reachable"
                .to_string()
        } else {
            "the radio enumerates but its device node is missing; reload udev with `sudo udevadm \
             control --reload-rules && sudo udevadm trigger` and replug it"
                .to_string()
        };
    }
    let group = blocked
        .iter()
        .find(|node| node.mode & 0o060 == 0o060)
        .map(|node| node.gid);
    match (container, group) {
        (true, Some(gid)) => format!(
            "the container user may not open this node. Give it the group that owns the node on \
             the host: `docker run --group-add {gid}`, or `group_add: [\"{gid}\"]` in Compose. \
             Running the container as root is the last resort."
        ),
        (true, None) => "the container user may not open this node, and its group may not \
             either. Install the receiver's udev rule on the host: vendor rules grant a group, \
             usually plugdev, then pass that group's numeric id with `--group-add`. Running the \
             container as root is the last resort."
            .to_string(),
        (false, Some(gid)) => format!(
            "join the group that owns the node: `sudo usermod -aG {gid} $USER`, then log in \
             again. Installing the receiver's vendor udev rule does the same for every radio of \
             that kind."
        ),
        (false, None) => "install the receiver's vendor udev rule (the rtl-sdr, hackrf, \
             libbladerf and libiio packages ship them), then `sudo udevadm control \
             --reload-rules && sudo udevadm trigger` and replug the radio."
            .to_string(),
    }
}

fn usb_permission_check(nodes: &[sdrmm_device::usb::RadioNode]) -> DoctorCheck {
    let user = current_user();
    if nodes.is_empty() {
        return DoctorCheck {
            id: "usb.permissions".to_string(),
            name: "USB permissions".to_string(),
            status: CheckStatus::Ok,
            detail: "no USB radio attached".to_string(),
            hint: None,
        };
    }
    let blocked: Vec<&sdrmm_device::usb::RadioNode> =
        nodes.iter().filter(|node| !node.openable).collect();
    let detail = nodes
        .iter()
        .map(|node| node_line(node, user))
        .collect::<Vec<_>>()
        .join("\n");
    if blocked.is_empty() {
        return DoctorCheck {
            id: "usb.permissions".to_string(),
            name: "USB permissions".to_string(),
            status: CheckStatus::Ok,
            detail,
            hint: None,
        };
    }
    DoctorCheck {
        id: "usb.permissions".to_string(),
        name: "USB permissions".to_string(),
        status: CheckStatus::Fail,
        detail,
        hint: Some(permission_hint(&blocked)),
    }
}

#[derive(Clone, Copy)]
enum PathKind {
    File,
    Directory,
}

fn path_check(
    id: &str,
    name: &str,
    path: Option<&Path>,
    kind: PathKind,
    absent_consequence: &str,
) -> DoctorCheck {
    let Some(path) = path else {
        return DoctorCheck {
            id: id.to_string(),
            name: name.to_string(),
            status: CheckStatus::Warn,
            detail: format!("not configured: {absent_consequence}"),
            hint: None,
        };
    };
    let dir = match kind {
        PathKind::File => path.parent().unwrap_or(path),
        PathKind::Directory => path,
    };
    let (status, detail, hint) = match writable(dir) {
        Ok(()) => (CheckStatus::Ok, path.display().to_string(), None),
        Err(e) => (
            CheckStatus::Fail,
            format!("{} is not writable: {e}", dir.display()),
            Some("fix the directory's ownership, or pass an explicit path".to_string()),
        ),
    };
    DoctorCheck {
        id: id.to_string(),
        name: name.to_string(),
        status,
        detail,
        hint,
    }
}

fn writable(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let probe = dir.join(".sdrmm-doctor-probe");
    std::fs::write(&probe, b"")?;
    std::fs::remove_file(&probe)
}

#[must_use]
pub fn render(report: &DoctorReport) -> String {
    let mut out = format!("SDRmm {} ({})\n\n", report.version, report.platform);
    for check in &report.checks {
        let mark = match check.status {
            CheckStatus::Ok => "ok  ",
            CheckStatus::Warn => "warn",
            CheckStatus::Fail => "FAIL",
        };
        out.push_str(&format!("[{mark}] {}\n", check.name));
        for line in check.detail.lines() {
            out.push_str(&format!("       {line}\n"));
        }
        if let Some(hint) = &check.hint {
            out.push_str(&format!("       → {hint}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(id: &str, status: CheckStatus, hint: Option<&str>) -> DoctorCheck {
        DoctorCheck {
            id: id.to_string(),
            name: format!("Check {id}"),
            status,
            detail: "line one\nline two".to_string(),
            hint: hint.map(str::to_string),
        }
    }

    fn node(name: &'static str, openable: bool, mode: u32) -> sdrmm_device::usb::RadioNode {
        sdrmm_device::usb::RadioNode {
            name,
            path: std::path::PathBuf::from("/dev/bus/usb/001/004"),
            present: true,
            uid: 0,
            gid: 46,
            mode,
            openable,
        }
    }

    #[test]
    fn an_rtl_sdr_known_by_its_port_is_told_to_get_a_serial() {
        let dongle = |key: &str, serial: Option<&str>| DeviceInfo {
            driver: DeviceBackend::Rtlsdr,
            key: key.to_string(),
            label: format!("RTL-SDR {key}"),
            serial: serial.map(str::to_string),
            profile: None,
        };
        let network = DeviceInfo {
            driver: DeviceBackend::Rtltcp,
            ..dongle("10.0.0.5:1234", None)
        };
        assert!(port_bound_check(&[dongle("00000123", Some("00000123")), network]).is_none());

        let warned =
            port_bound_check(&[dongle("1/4", None), dongle("1/5", None)]).expect("a warning");
        assert_eq!(warned.status, CheckStatus::Warn);
        assert!(warned.detail.contains("RTL-SDR 1/4\nRTL-SDR 1/5"));
        assert!(
            warned
                .hint
                .is_some_and(|hint| hint.contains("rtl_eeprom -s"))
        );
    }

    #[test]
    fn a_radio_this_user_may_open_passes_the_check() {
        let check = usb_permission_check(&[node("RTL-SDR", true, 0o666)]);
        assert_eq!(check.status, CheckStatus::Ok);
        assert!(check.detail.contains("can be opened"), "{}", check.detail);
        assert!(check.hint.is_none());
    }

    #[test]
    fn a_radio_this_user_may_not_open_fails_and_names_its_node() {
        let check = usb_permission_check(&[node("bladeRF 2.0 micro", false, 0o660)]);
        assert_eq!(check.status, CheckStatus::Fail);
        assert!(
            check
                .detail
                .contains("bladeRF 2.0 micro: /dev/bus/usb/001/004 cannot be opened"),
            "{}",
            check.detail
        );
        assert!(
            check.detail.contains("owner 0:46, mode 0660"),
            "{}",
            check.detail
        );
        assert!(check.hint.is_some());
    }

    #[test]
    fn a_bus_with_no_radio_on_it_is_not_a_permission_problem() {
        let check = usb_permission_check(&[]);
        assert_eq!(check.status, CheckStatus::Ok);
        assert_eq!(check.detail, "no USB radio attached");
    }

    #[test]
    fn a_node_whose_group_may_open_it_is_answered_with_that_group() {
        let blocked = node("bladeRF 2.0 micro", false, 0o660);
        let hint = permission_hint(&[&blocked]);
        assert!(hint.contains("46"), "{hint}");
    }

    #[test]
    fn a_node_no_group_may_open_is_answered_with_udev() {
        let blocked = node("bladeRF 2.0 micro", false, 0o600);
        let hint = permission_hint(&[&blocked]);
        assert!(hint.contains("udev"), "{hint}");
    }

    #[test]
    fn a_missing_node_says_the_bus_never_arrived() {
        let mut missing = node("HackRF One", false, 0);
        missing.present = false;
        let check = usb_permission_check(std::slice::from_ref(&missing));
        assert_eq!(check.status, CheckStatus::Fail);
        assert!(
            check.detail.contains("is not visible to this process"),
            "{}",
            check.detail
        );
    }

    #[test]
    fn a_rate_the_radio_does_not_hold_fails_the_check() {
        let held = [
            (2_048_000.0, Some(2_048_000.0)),
            (2_400_000.0, Some(2_286_826.0)),
            (3_200_000.0, None),
        ];
        let check = rate_check("soapy:00000001", "Generic RTL2832U", &held, &[]);
        assert_eq!(check.status, CheckStatus::Fail);
        assert!(check.detail.contains("2048000 Hz  ok"), "{}", check.detail);
        assert!(
            check.detail.contains("2400000 Hz  HELD 2286826 Hz"),
            "{}",
            check.detail
        );
        assert!(
            check.detail.contains("3200000 Hz  refused"),
            "{}",
            check.detail
        );
        assert!(check.hint.is_some());
    }

    #[test]
    fn a_radio_that_holds_every_advertised_rate_passes() {
        let held = [(2_048_000.0, Some(2_048_000.0)), (1_024_000.0, None)];
        assert_eq!(
            rate_check("x", "X", &held[..1], &[]).status,
            CheckStatus::Ok,
            "an exact read-back is not a mismatch"
        );
        assert_eq!(rate_check("x", "X", &held, &[]).status, CheckStatus::Fail);
        assert_eq!(rate_check("x", "X", &[], &[]).status, CheckStatus::Warn);
    }

    #[test]
    fn a_radio_that_resamples_across_a_window_has_nothing_to_fail() {
        let window = sdrmm_wire::Range {
            min: 2e6,
            max: 20e6,
            step: None,
        };
        let check = rate_check("hackrf:1", "HackRF One", &[], &[window]);
        assert_eq!(
            check.status,
            CheckStatus::Ok,
            "a continuous radio is not a broken one"
        );
        assert!(
            check.detail.contains("2000000-20000000 Hz"),
            "{}",
            check.detail
        );
        assert!(check.hint.is_none());
    }

    #[test]
    fn render_lists_every_check_with_its_detail_lines_and_hints() {
        let report = DoctorReport {
            version: "1.2.3".to_string(),
            platform: "linux/aarch64".to_string(),
            checks: vec![
                check("a", CheckStatus::Ok, None),
                check("b", CheckStatus::Warn, Some("do the thing")),
                check("c", CheckStatus::Fail, None),
            ],
        };
        let text = render(&report);
        assert!(text.starts_with("SDRmm 1.2.3 (linux/aarch64)"));
        assert!(text.contains("[ok  ] Check a"));
        assert!(text.contains("[warn] Check b"));
        assert!(text.contains("[FAIL] Check c"));
        assert_eq!(text.matches("       line one").count(), 3);
        assert_eq!(text.matches("       line two").count(), 3);
        assert!(text.contains("       → do the thing"));
    }

    #[test]
    fn path_check_reports_absence_and_writability() {
        let absent = path_check("x", "X", None, PathKind::File, "nothing persists");
        assert_eq!(absent.status, CheckStatus::Warn);
        assert!(absent.detail.contains("nothing persists"));

        let dir = tempfile::TempDir::new().expect("tempdir");
        let ok = path_check("x", "X", Some(dir.path()), PathKind::Directory, "unused");
        assert_eq!(ok.status, CheckStatus::Ok, "{}", ok.detail);

        let file = dir.path().join("sub").join("sdrmm.db");
        let ok = path_check("x", "X", Some(&file), PathKind::File, "unused");
        assert_eq!(ok.status, CheckStatus::Ok, "{}", ok.detail);
        assert!(file.parent().is_some_and(Path::exists));
        assert!(
            !file.exists(),
            "the probe must not create the database itself"
        );
    }

    #[test]
    fn path_kind_is_told_not_guessed() {
        let root = tempfile::TempDir::new().expect("tempdir");
        let db = root.path().join("sdrmm");
        let ok = path_check("db", "DB", Some(&db), PathKind::File, "unused");
        assert_eq!(ok.status, CheckStatus::Ok, "{}", ok.detail);
        assert!(
            !db.exists(),
            "an extension-less db path must not become a directory"
        );

        let recordings = root.path().join("sdr.captures");
        let ok = path_check(
            "rec",
            "Rec",
            Some(&recordings),
            PathKind::Directory,
            "unused",
        );
        assert_eq!(ok.status, CheckStatus::Ok, "{}", ok.detail);
        assert!(
            recordings.is_dir(),
            "a dotted directory must be probed itself"
        );
    }

    #[test]
    fn the_report_carries_what_a_driver_says_about_its_runtime() {
        struct Ailing;

        impl sdrmm_device::DeviceDriver for Ailing {
            fn id(&self) -> DeviceBackend {
                DeviceBackend::Sdrplay
            }

            fn probe(&self) -> Vec<DeviceInfo> {
                Vec::new()
            }

            fn open(
                &self,
                _info: &DeviceInfo,
            ) -> Result<Box<dyn sdrmm_device::SdrDevice>, sdrmm_device::DeviceError> {
                Err(sdrmm_device::DeviceError::NotFound("ailing".to_string()))
            }

            fn health(&self) -> Option<DoctorCheck> {
                Some(DoctorCheck {
                    id: "ailing.library".to_string(),
                    name: "Ailing SDK".to_string(),
                    status: CheckStatus::Warn,
                    detail: "not installed".to_string(),
                    hint: None,
                })
            }
        }

        let mut registry = sdrmm_device::DeviceRegistry::new();
        registry.register(10, Box::new(Ailing));
        let report = report(&registry, None, None);
        let runtime = report
            .checks
            .iter()
            .find(|check| check.id == "ailing.library")
            .expect("the driver's own check");
        assert_eq!(runtime.status, CheckStatus::Warn);
    }

    #[test]
    fn backends_check_warns_when_only_the_virtual_driver_is_compiled_in() {
        let mut registry = sdrmm_device::DeviceRegistry::new();
        registry.register(10, Box::new(sdrmm_device_virtual::VirtualDriver::new()));
        let check = backends_check(&registry);
        assert_eq!(check.status, CheckStatus::Warn);
        assert!(check.detail.contains("virtual"));
        assert!(check.hint.is_some_and(|h| h.contains("soapy")));
    }

    #[test]
    fn virtual_backend_description_matches_the_build_policy() {
        assert_eq!(
            virtual_capabilities(true),
            "synthetic signal-generator and marker radios, plus SigMF playback through the \
             virtual driver"
        );
        assert_eq!(
            virtual_capabilities(false),
            "SigMF playback through the virtual driver"
        );
    }

    fn lane(name: &str, lane: u32, lanes: u32) -> DeviceInfo {
        DeviceInfo {
            driver: DeviceBackend::Rtlsdr,
            key: (1000 + lane).to_string(),
            label: format!("{name} lane {}", lane + 1),
            serial: None,
            profile: Some(sdrmm_wire::DeviceProfile {
                rx_streams: 1,
                bank: Some(sdrmm_wire::ClockBank {
                    id: name.to_string(),
                    lane,
                    lanes,
                }),
                ..sdrmm_wire::DeviceProfile::default()
            }),
        }
    }

    fn bank(name: &str, lanes: u32, seen: u32) -> Vec<DeviceInfo> {
        (0..seen).map(|held| lane(name, held, lanes)).collect()
    }

    #[test]
    fn a_whole_bank_lists_its_lanes() {
        let mut devices = bank("KrakenSDR 01/1", 5, 5);
        devices.extend(bank("KerberosSDR 02/3", 4, 4));
        let check = banks_check(&devices).expect("banks are listed");
        assert_eq!(check.status, CheckStatus::Ok);
        assert_eq!(
            check.detail,
            "KrakenSDR 01/1: 5 of 5 lanes\nKerberosSDR 02/3: 4 of 4 lanes"
        );
        assert!(check.hint.is_none());
        assert!(banks_check(&[]).is_none(), "no bank, no line");
    }

    #[test]
    fn a_bank_short_of_a_dongle_is_a_warning() {
        let check = banks_check(&bank("KrakenSDR 01/1", 5, 4)).expect("listed");
        assert_eq!(check.status, CheckStatus::Warn);
        assert_eq!(check.detail, "KrakenSDR 01/1: 4 of 5 lanes");
        assert!(check.hint.is_some());
    }

    #[test]
    fn arrays_report_their_sync_and_cal() {
        let locked = ArrayStatus {
            node: "roof".to_string(),
            sync: SyncState::Locked,
            cal: sdrmm_wire::CalPhase::Solved,
            ..ArrayStatus::default()
        };
        let check = arrays_check(std::slice::from_ref(&locked)).expect("one array");
        assert_eq!(check.status, CheckStatus::Ok);
        assert_eq!(check.detail, "array roof: Locked Calibrated");

        let lost = ArrayStatus {
            node: "mast".to_string(),
            sync: SyncState::Lost,
            failure: Some(sdrmm_wire::ArrayFailure::DeviceDown { lane: 2 }),
            ..ArrayStatus::default()
        };
        let check = arrays_check(&[locked, lost]).expect("two arrays");
        assert_eq!(check.status, CheckStatus::Warn);
        assert_eq!(
            check.detail,
            "array roof: Locked Calibrated\narray mast: Lost No cal (Radio 3 down)"
        );
        assert!(arrays_check(&[]).is_none());
    }
}
