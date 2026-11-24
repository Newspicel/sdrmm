use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Value, json};

use super::stage::Staged;
use super::{IDENTIFIER, Os, Platform};

const DESCRIPTION: &str = "Modular software-defined radio";
const LONG_DESCRIPTION: &str = "Modular, client-server software-defined radio receiver.";
const HOMEPAGE: &str = "https://sdrmm.com";
const AUTHOR: &str = "Julian Haag";
/// linuxdeploy sets the launcher's rpath to `$ORIGIN/../lib`, so the `AppImage` carries Chromium
/// in `usr/lib`, not in `usr/lib/sdrmm-app` like the deb.
const APPIMAGE_RUNTIME: &str = "usr/lib";
/// Chromium's shared libraries, already in the `AppImage`'s `usr/lib`, which linuxdeploy must
/// leave as CEF ships them.
const CHROMIUM_LIBRARIES: &[&str] = &["libcef.so", "libvk_swiftshader.so", "libvulkan.so.1"];
/// What Chromium links on Debian and Ubuntu; the `t64` names are Ubuntu 24.04's.
const DEB_DEPENDS: &[&str] = &[
    "libasound2t64 | libasound2",
    "libatk-bridge2.0-0t64 | libatk-bridge2.0-0",
    "libatk1.0-0t64 | libatk1.0-0",
    "libatspi2.0-0t64 | libatspi2.0-0",
    "libcairo2",
    "libcups2t64 | libcups2",
    "libdbus-1-3",
    "libdrm2",
    "libexpat1",
    "libgbm1",
    "libglib2.0-0t64 | libglib2.0-0",
    "libgtk-3-0t64 | libgtk-3-0",
    "libnspr4",
    "libnss3",
    "libpango-1.0-0",
    "libx11-6",
    "libxcb1",
    "libxcomposite1",
    "libxdamage1",
    "libxext6",
    "libxfixes3",
    "libxkbcommon0",
    "libxrandr2",
    "xdg-utils",
];

pub fn write(
    root: &Path,
    platform: &Platform,
    staged: &Staged,
    stage: &Path,
    release: &Path,
) -> Result<PathBuf> {
    let config = config(root, platform, staged, &release.join("packages"));
    let path = stage.join("packager.json");
    std::fs::write(&path, serde_json::to_string_pretty(&config)?)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

fn config(root: &Path, platform: &Platform, staged: &Staged, out: &Path) -> Value {
    let desktop = root.join("apps/desktop");
    let mut config = json!({
        "productName": platform.product(),
        "version": env!("CARGO_PKG_VERSION"),
        "identifier": IDENTIFIER,
        "description": DESCRIPTION,
        "longDescription": LONG_DESCRIPTION,
        "homepage": HOMEPAGE,
        "authors": [AUTHOR],
        "publisher": AUTHOR,
        "category": "Utility",
        "outDir": out,
        "binariesDir": staged.bin_dir,
        "binaries": [{ "path": staged.main, "main": true }],
        "targetTriple": platform.triple,
        "icons": [
            desktop.join("icons/32x32.png"),
            desktop.join("icons/128x128.png"),
            desktop.join("icons/128x128@2x.png"),
            desktop.join("icons/icon.icns"),
            desktop.join("icons/icon.ico"),
        ],
    });
    let extra = match platform.os {
        Os::Mac => mac(root, staged),
        Os::Linux => linux(root, platform, staged),
        Os::Windows => windows(staged),
    };
    merge(&mut config, extra);
    config
}

fn mac(root: &Path, staged: &Staged) -> Value {
    let desktop = root.join("apps/desktop");
    let mut macos = json!({
        "frameworks": staged.frameworks,
        "entitlements": desktop.join("entitlements.plist"),
        "infoPlistPath": desktop.join("Info.plist"),
        "minimumSystemVersion": super::mac::MINIMUM_SYSTEM,
    });
    if let Ok(identity) = std::env::var("APPLE_SIGNING_IDENTITY") {
        macos["signingIdentity"] = Value::from(identity);
    }
    json!({ "macos": macos, "resources": staged.resources })
}

fn linux(root: &Path, platform: &Platform, staged: &Staged) -> Value {
    json!({
        "deb": {
            "depends": DEB_DEPENDS,
            "section": "hamradio",
            "desktopTemplate": root.join("packaging/linux/sdrmm-app.desktop"),
            "files": runtime_at(staged, &format!("usr/lib/{}", platform.product())),
        },
        "appimage": {
            "files": runtime_at(staged, APPIMAGE_RUNTIME),
            "excludedLibs": CHROMIUM_LIBRARIES,
        },
    })
}

fn windows(staged: &Staged) -> Value {
    json!({ "resources": runtime(staged) })
}

/// Everything beside `libcef`, at the root of the resource directory.
fn runtime(staged: &Staged) -> Value {
    json!([{ "src": staged.runtime, "target": "." }])
}

/// Everything beside `libcef`, copied into a Linux package at `target`.
fn runtime_at(staged: &Staged, target: &str) -> Value {
    let mut files = serde_json::Map::new();
    if let Some(runtime) = &staged.runtime {
        files.insert(runtime.to_string_lossy().into_owned(), Value::from(target));
    }
    Value::Object(files)
}

fn merge(into: &mut Value, extra: Value) {
    if let (Some(into), Value::Object(extra)) = (into.as_object_mut(), extra) {
        into.extend(extra);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn staged(runtime: Option<&str>) -> Staged {
        Staged {
            bin_dir: PathBuf::from("/stage/bin"),
            main: "sdrmm-app".to_owned(),
            runtime: runtime.map(PathBuf::from),
            frameworks: vec![PathBuf::from("/cef/Chromium Embedded Framework.framework")],
            resources: Vec::new(),
        }
    }

    #[test]
    fn linux_installs_chromium_where_the_launcher_finds_it() {
        let platform = Platform::of("x86_64-unknown-linux-gnu").unwrap();
        let config = config(
            Path::new("/repo"),
            &platform,
            &staged(Some("/stage/runtime")),
            Path::new("/out"),
        );
        assert_eq!(config["productName"], "sdrmm-app");
        assert!(config.get("resources").is_none());
        assert_eq!(
            config["deb"]["files"]["/stage/runtime"],
            "usr/lib/sdrmm-app"
        );
        assert_eq!(config["appimage"]["files"]["/stage/runtime"], "usr/lib");
        assert_eq!(config["binaries"][0]["path"], "sdrmm-app");
        assert!(
            config["deb"]["depends"]
                .as_array()
                .is_some_and(|d| !d.is_empty())
        );
    }

    #[test]
    fn macos_carries_chromium_as_a_framework() {
        let platform = Platform::of("aarch64-apple-darwin").unwrap();
        let config = config(
            Path::new("/repo"),
            &platform,
            &staged(None),
            Path::new("/out"),
        );
        assert_eq!(
            config["macos"]["frameworks"][0],
            "/cef/Chromium Embedded Framework.framework"
        );
        assert_eq!(
            config["macos"]["minimumSystemVersion"],
            super::super::mac::MINIMUM_SYSTEM
        );
    }
}
