//! What the Windows installers clean up from a Tauri release they update. Tauri named the
//! executable `sdrmm-desktop.exe` and derived the MSI upgrade code from it; `cargo packager`
//! installs `SDRmm.exe` under a code of its own, so neither installer replaces the old app
//! unless told to.

use serde_json::{Value, json};

/// Tauri's MSI upgrade code: UUIDv5 of `SDRmm.exe.app.x64` in the DNS namespace.
const MSI_UPGRADE_CODE: &str = "e6a896fe-a81d-5161-8737-cb55a6f08700";
const MSI_COMPONENT_GROUP: &str = "TauriRelease";

/// Lists the Tauri product in the Upgrade table, so the major upgrade removes it.
fn msi_fragment() -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<Wix xmlns="http://schemas.microsoft.com/wix/2006/wi">
  <Fragment>
    <Upgrade Id="{MSI_UPGRADE_CODE}">
      <UpgradeVersion Minimum="0.0.0" IncludeMinimum="yes" Property="TAURIRELEASE" />
    </Upgrade>
    <ComponentGroup Id="{MSI_COMPONENT_GROUP}" />
  </Fragment>
</Wix>
"#
    )
}

/// Removes the Tauri executable and its Start Menu shortcut, which sat in the menu's root
/// where `cargo packager` uses a folder.
const NSIS_SECTION: &str = r#"Section TauriRelease
  Delete "$INSTDIR\sdrmm-desktop.exe"
  Delete "$SMPROGRAMS\SDRmm.lnk"
SectionEnd
"#;

pub fn windows() -> Value {
    json!({
        "wix": {
            "fragments": [msi_fragment()],
            "componentGroupRefs": [MSI_COMPONENT_GROUP],
        },
        "nsis": { "preinstallSection": NSIS_SECTION },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_msi_removes_the_tauri_product() {
        let config = windows();
        let fragment = config["wix"]["fragments"][0].as_str().unwrap();
        assert!(fragment.contains(&format!(r#"<Upgrade Id="{MSI_UPGRADE_CODE}">"#)));
        assert!(fragment.contains(&format!(r#"<ComponentGroup Id="{MSI_COMPONENT_GROUP}" />"#)));
        assert_eq!(config["wix"]["componentGroupRefs"][0], MSI_COMPONENT_GROUP);
    }

    #[test]
    fn nsis_removes_the_tauri_executable_and_shortcut() {
        let section = windows()["nsis"]["preinstallSection"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(section.contains(r#"Delete "$INSTDIR\sdrmm-desktop.exe""#));
        assert!(section.contains(r#"Delete "$SMPROGRAMS\SDRmm.lnk""#));
    }
}
