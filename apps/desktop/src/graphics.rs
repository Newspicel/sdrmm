use std::path::Path;

const MODE: &str = "SDRMM_LINUX_GRAPHICS";
const DMABUF: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";
const COMPOSITING: &str = "WEBKIT_DISABLE_COMPOSITING_MODE";
const EXPLICIT_SYNC: &str = "__NV_DISABLE_EXPLICIT_SYNC";
const DRM_CLASS: &str = "/sys/class/drm";
const NVIDIA_VENDOR: &str = "0x10de";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Auto,
    Safe,
    Off,
}

impl std::str::FromStr for Mode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "safe" => Ok(Self::Safe),
            "off" => Ok(Self::Off),
            other => Err(other.to_owned()),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Stack {
    nvidia_screen: bool,
    wayland: bool,
    strict_sync: bool,
}

fn quirks(mode: Mode, stack: Stack) -> &'static [&'static str] {
    match mode {
        Mode::Off => &[],
        Mode::Safe => &[DMABUF, COMPOSITING],
        Mode::Auto if !stack.nvidia_screen => &[],
        Mode::Auto if stack.wayland && !stack.strict_sync => &[EXPLICIT_SYNC],
        Mode::Auto => &[DMABUF],
    }
}

fn mode() -> Mode {
    let Ok(value) = std::env::var(MODE) else {
        return Mode::default();
    };
    value.parse().unwrap_or_else(|unknown| {
        tracing::warn!("{MODE}={unknown} is not one of auto, safe, off; reading it as auto");
        Mode::default()
    })
}

fn card_of(connector: &str) -> Option<&str> {
    let (card, output) = connector.split_once('-')?;
    (card.starts_with("card") && !output.is_empty()).then_some(card)
}

fn reads(path: &Path, expected: &str) -> bool {
    std::fs::read_to_string(path).is_ok_and(|value| value.trim().eq_ignore_ascii_case(expected))
}

fn nvidia_screen(drm: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(drm) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name();
        let Some(card) = card_of(&name.to_string_lossy()).map(str::to_owned) else {
            return false;
        };
        reads(&entry.path().join("status"), "connected")
            && reads(&drm.join(card).join("device/vendor"), NVIDIA_VENDOR)
    })
}

fn wayland(session_type: Option<&str>, display: Option<&str>) -> bool {
    match session_type {
        Some(value) if value.eq_ignore_ascii_case("wayland") => true,
        Some(value) if value.eq_ignore_ascii_case("x11") => false,
        _ => display.is_some_and(|value| !value.is_empty()),
    }
}

fn strict_sync(desktop: Option<&str>) -> bool {
    desktop.is_some_and(|value| value.to_ascii_lowercase().contains("hyprland"))
}

fn stack() -> Stack {
    let session_type = std::env::var("XDG_SESSION_TYPE").ok();
    let display = std::env::var("WAYLAND_DISPLAY").ok();
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();
    Stack {
        nvidia_screen: nvidia_screen(Path::new(DRM_CLASS)),
        wayland: wayland(session_type.as_deref(), display.as_deref()),
        strict_sync: strict_sync(desktop.as_deref()),
    }
}

/// # Safety
/// The caller must invoke this during single-threaded process startup, before any other thread
/// can read or write the process environment and before the first webview is built, which is what
/// reads these.
pub unsafe fn configure() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let mode = mode();
    let stack = stack();
    let mut applied = Vec::new();
    for name in quirks(mode, stack) {
        if std::env::var_os(name).is_some() {
            continue;
        }
        unsafe { std::env::set_var(name, "1") };
        applied.push(*name);
    }
    tracing::info!(?mode, ?stack, ?applied, "linux webview rendering");
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTEL: Stack = Stack {
        nvidia_screen: false,
        wayland: false,
        strict_sync: false,
    };
    const NVIDIA_X11: Stack = Stack {
        nvidia_screen: true,
        ..INTEL
    };
    const NVIDIA_WAYLAND: Stack = Stack {
        nvidia_screen: true,
        wayland: true,
        strict_sync: false,
    };

    fn screen(dir: &Path, connector: &str, status: &str, vendor: &str) {
        let card = card_of(connector).expect("a connector names its card");
        std::fs::create_dir_all(dir.join(connector)).expect("connector directory");
        std::fs::write(dir.join(connector).join("status"), status).expect("connector status");
        std::fs::create_dir_all(dir.join(card).join("device")).expect("card directory");
        std::fs::write(dir.join(card).join("device/vendor"), vendor).expect("card vendor");
    }

    #[test]
    fn auto_leaves_a_working_stack_accelerated() {
        assert!(quirks(Mode::Auto, INTEL).is_empty());
    }

    #[test]
    fn auto_keeps_the_dmabuf_renderer_on_nvidia_wayland_and_drops_explicit_sync() {
        assert_eq!(quirks(Mode::Auto, NVIDIA_WAYLAND), [EXPLICIT_SYNC]);
    }

    #[test]
    fn auto_drops_the_dmabuf_renderer_for_an_nvidia_screen_on_x11() {
        assert_eq!(quirks(Mode::Auto, NVIDIA_X11), [DMABUF]);
    }

    #[test]
    fn auto_drops_the_dmabuf_renderer_where_the_compositor_demands_an_acquire_point() {
        let stack = Stack {
            strict_sync: true,
            ..NVIDIA_WAYLAND
        };
        assert_eq!(quirks(Mode::Auto, stack), [DMABUF]);
    }

    #[test]
    fn safe_drops_compositing_as_well() {
        assert_eq!(quirks(Mode::Safe, INTEL), [DMABUF, COMPOSITING]);
    }

    #[test]
    fn off_keeps_every_path_even_where_auto_would_act() {
        assert!(quirks(Mode::Off, NVIDIA_X11).is_empty());
    }

    #[test]
    fn mode_reads_the_documented_spellings() {
        assert_eq!(" AUTO ".parse(), Ok(Mode::Auto));
        assert_eq!("safe".parse(), Ok(Mode::Safe));
        assert_eq!("off".parse(), Ok(Mode::Off));
        assert_eq!("fast".parse::<Mode>(), Err("fast".to_owned()));
    }

    #[test]
    fn a_connected_nvidia_output_names_the_screen_nvidia() {
        let drm = tempfile::tempdir().expect("temporary drm class");
        screen(drm.path(), "card1-DP-1", "connected", "0x10de");
        assert!(nvidia_screen(drm.path()));
    }

    #[test]
    fn an_nvidia_card_driving_no_display_is_not_the_screen() {
        let drm = tempfile::tempdir().expect("temporary drm class");
        screen(drm.path(), "card0-eDP-1", "connected", "0x8086");
        screen(drm.path(), "card1-DP-1", "disconnected", "0x10de");
        assert!(!nvidia_screen(drm.path()));
    }

    #[test]
    fn a_missing_drm_class_claims_nothing() {
        assert!(!nvidia_screen(Path::new("/nonexistent/class/drm")));
    }

    #[test]
    fn card_of_names_the_card_behind_an_output_and_nothing_else() {
        assert_eq!(card_of("card0-HDMI-A-1"), Some("card0"));
        assert_eq!(card_of("card0"), None);
        assert_eq!(card_of("card0-"), None);
        assert_eq!(card_of("renderD128"), None);
    }

    #[test]
    fn the_session_type_wins_over_a_stale_wayland_socket() {
        assert!(!wayland(Some("x11"), Some("wayland-0")));
        assert!(wayland(Some("Wayland"), None));
        assert!(wayland(None, Some("wayland-0")));
        assert!(!wayland(None, None));
        assert!(!wayland(None, Some("")));
    }

    #[test]
    fn strict_sync_names_the_compositors_that_reject_a_missing_acquire_point() {
        assert!(strict_sync(Some("Hyprland")));
        assert!(strict_sync(Some("hyprland:wlroots")));
        assert!(!strict_sync(Some("KDE")));
        assert!(!strict_sync(None));
    }
}
