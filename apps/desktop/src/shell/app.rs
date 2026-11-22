use cef::*;

use super::{Start, window};

/// Chromium switches for the browser process. Audio starts without a click, as a desktop app's
/// does; the one page needs no spare renderer kept warm, and Skia's older GPU backend holds some
/// 450 MB less than Graphite for the same page at the same CPU; macOS never asks for Keychain
/// access, since nothing here stores a password; and Linux picks Wayland or X11 by the session
/// instead of always X11.
const SWITCHES: &[(&str, &str)] = &[
    ("autoplay-policy", "no-user-gesture-required"),
    (
        "disable-features",
        "SpareRendererForSitePerProcess,SkiaGraphite",
    ),
    #[cfg(target_os = "macos")]
    ("use-mock-keychain", ""),
    #[cfg(target_os = "linux")]
    ("ozone-platform-hint", "auto"),
];

/// Switches that hold a list CEF fills with its own entries. Ours join them: replacing CEF's
/// `disable-features` crashes the browser as it opens the window.
const LISTS: &[&str] = &["disable-features"];

wrap_app! {
    pub struct ShellApp {
        start: Start,
    }

    impl App {
        fn on_before_command_line_processing(
            &self,
            process_type: Option<&CefString>,
            command_line: Option<&mut CommandLine>,
        ) {
            let browser = process_type.is_none_or(|kind| kind.to_string().is_empty());
            let Some(command_line) = command_line.filter(|_| browser) else {
                return;
            };
            for (name, value) in SWITCHES {
                let switch = CefString::from(*name);
                if value.is_empty() {
                    command_line.append_switch(Some(&switch));
                    continue;
                }
                let held = (LISTS.contains(name) && command_line.has_switch(Some(&switch)) == 1)
                    .then(|| CefString::from(&command_line.switch_value(Some(&switch))).to_string());
                let value = CefString::from(joined(held.as_deref(), value).as_str());
                command_line.append_switch_with_value(Some(&switch), Some(&value));
            }
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(ShellProcess::new(self.start.clone()))
        }
    }
}

wrap_browser_process_handler! {
    struct ShellProcess {
        start: Start,
    }

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            name_audio_outputs(&self.start);
            window::open(&self.start);
        }
    }
}

fn joined(held: Option<&str>, value: &str) -> String {
    match held {
        Some(held) if !held.is_empty() => format!("{held},{value}"),
        _ => value.to_owned(),
    }
}

/// Chromium names audio output devices only to a page that may use the microphone. Granting it
/// to the app's own origin lets each Speaker list its outputs by name without opening a mic.
fn name_audio_outputs(start: &Start) {
    let Some(context) = request_context_get_global_context() else {
        tracing::warn!("no browser context: audio outputs stay unnamed");
        return;
    };
    let origin = CefString::from(start.url.origin().ascii_serialization().as_str());
    context.set_content_setting(
        Some(&origin),
        Some(&origin),
        ContentSettingTypes::MEDIASTREAM_MIC,
        ContentSettingValues::ALLOW,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_feature_list_keeps_what_cef_put_there() {
        assert_eq!(joined(Some("LensOverlay"), "Spare"), "LensOverlay,Spare");
        assert_eq!(joined(Some(""), "Spare"), "Spare");
        assert_eq!(joined(None, "Spare"), "Spare");
    }
}
