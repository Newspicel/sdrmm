use std::path::{Path, PathBuf};

use anyhow::Result;

use super::IDENTIFIER;
use super::stage::copy;
use crate::pack::copy_tree;

pub const FRAMEWORK: &str = "Chromium Embedded Framework.framework";
pub const MINIMUM_SYSTEM: &str = "12.0";
/// Chromium starts each kind of child process from its own helper app, found by name.
const HELPERS: [(&str, &str); 5] = [
    ("", "helper"),
    (" (GPU)", "helper.gpu"),
    (" (Renderer)", "helper.renderer"),
    (" (Plugin)", "helper.plugin"),
    (" (Alerts)", "helper.alerts"),
];

pub fn helpers(helper: &Path, stage: &Path, product: &str) -> Result<Vec<PathBuf>> {
    let dir = stage.join("helpers");
    HELPERS
        .iter()
        .map(|(suffix, id)| {
            let name = format!("{product} Helper{suffix}");
            let identifier = format!("{IDENTIFIER}.{id}");
            bundle(&dir, &name, &identifier, helper, true)
        })
        .collect()
}

/// An unsigned app bundle that runs from the build tree. Chromium is copied in, not linked:
/// the helpers' sandbox only reads inside the bundle.
pub fn dev_app(release: &Path, cef: &Path, stage: &Path, product: &str) -> Result<PathBuf> {
    let app = bundle(
        stage,
        product,
        IDENTIFIER,
        &release.join("sdrmm-desktop"),
        false,
    )?;
    let frameworks = app.join("Contents/Frameworks");
    std::fs::create_dir_all(&frameworks)?;
    copy_tree(&cef.join(FRAMEWORK), &frameworks.join(FRAMEWORK))?;
    for helper in helpers(&release.join("sdrmm-desktop-helper"), stage, product)? {
        if let Some(name) = helper.file_name() {
            std::fs::rename(&helper, frameworks.join(name))?;
        }
    }
    Ok(app.join("Contents/MacOS").join(product))
}

fn bundle(dir: &Path, name: &str, identifier: &str, exe: &Path, helper: bool) -> Result<PathBuf> {
    let app = dir.join(format!("{name}.app"));
    let macos = app.join("Contents/MacOS");
    std::fs::create_dir_all(&macos)?;
    std::fs::write(
        app.join("Contents/Info.plist"),
        info_plist(name, identifier, helper),
    )?;
    copy(exe, &macos.join(name))?;
    Ok(app)
}

fn info_plist(executable: &str, identifier: &str, helper: bool) -> String {
    let version = env!("CARGO_PKG_VERSION");
    let background = if helper {
        "\n\t<key>LSUIElement</key>\n\t<string>1</string>"
    } else {
        ""
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>English</string>
	<key>CFBundleExecutable</key>
	<string>{executable}</string>
	<key>CFBundleIdentifier</key>
	<string>{identifier}</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>{executable}</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>{version}</string>
	<key>CFBundleVersion</key>
	<string>{version}</string>
	<key>LSEnvironment</key>
	<dict>
		<key>MallocNanoZone</key>
		<string>0</string>
	</dict>
	<key>LSMinimumSystemVersion</key>
	<string>{MINIMUM_SYSTEM}</string>
	<key>NSHighResolutionCapable</key>
	<true/>
	<key>NSSupportsAutomaticGraphicsSwitching</key>
	<true/>{background}
</dict>
</plist>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_carry_the_names_chromium_looks_for() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("helper");
        std::fs::write(&exe, b"").unwrap();
        let apps = helpers(&exe, dir.path(), "SDRmm").unwrap();
        let renderer = dir.path().join("helpers/SDRmm Helper (Renderer).app");
        assert!(apps.contains(&renderer));
        assert!(
            renderer
                .join("Contents/MacOS/SDRmm Helper (Renderer)")
                .is_file()
        );
        let plist = std::fs::read_to_string(renderer.join("Contents/Info.plist")).unwrap();
        assert!(plist.contains("<string>dev.newspicel.sdrmm.helper.renderer</string>"));
        assert!(plist.contains("LSUIElement"));
    }
}
