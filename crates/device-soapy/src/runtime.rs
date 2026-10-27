use std::path::Path;

use sdrmm_wire::{CheckStatus, DoctorCheck};

use crate::soapy;

#[derive(Clone, Debug)]
pub struct RuntimeInfo {
    pub core_version: Option<String>,
    pub library: Option<String>,
    pub search_paths: Vec<String>,
    pub modules: Vec<String>,
    pub error: Option<String>,
}

#[must_use]
pub fn runtime_info() -> RuntimeInfo {
    match soapy::installed() {
        Ok((version, path)) => RuntimeInfo {
            core_version: Some(version),
            library: Some(path.display().to_string()),
            search_paths: soapy::module_search_paths(),
            modules: soapy::list_modules(),
            error: None,
        },
        Err(error) => RuntimeInfo {
            core_version: None,
            library: None,
            search_paths: Vec::new(),
            modules: Vec::new(),
            error: Some(error),
        },
    }
}

#[must_use]
pub fn health(info: &RuntimeInfo) -> DoctorCheck {
    let Some(core_version) = &info.core_version else {
        return DoctorCheck {
            id: "soapy.runtime".to_string(),
            name: "SoapySDR runtime".to_string(),
            status: CheckStatus::Warn,
            detail: info
                .error
                .clone()
                .unwrap_or_else(|| "not installed".to_string()),
            hint: Some(
                "SoapySDR is optional. Install it with your package manager to reach bladeRF, \
                 LimeSDR and other hardware only its modules drive; RTL-SDR, HackRF, Airspy, \
                 AD936x, SDRplay and CR-8 receivers do not need it."
                    .to_string(),
            ),
        };
    };
    let module_names: Vec<String> = info
        .modules
        .iter()
        .map(|path| {
            Path::new(path)
                .file_name()
                .map_or_else(|| path.clone(), |name| name.to_string_lossy().into_owned())
        })
        .collect();
    DoctorCheck {
        id: "soapy.runtime".to_string(),
        name: "SoapySDR runtime".to_string(),
        status: if module_names.is_empty() {
            CheckStatus::Warn
        } else {
            CheckStatus::Ok
        },
        detail: format!(
            "core: {core_version}\nlibrary: {}\nmodule search path: {}\nloaded modules: {}",
            info.library.as_deref().unwrap_or("(unknown)"),
            if info.search_paths.is_empty() {
                "(none)".to_string()
            } else {
                info.search_paths.join(", ")
            },
            if module_names.is_empty() {
                "(none)".to_string()
            } else {
                module_names.join(", ")
            }
        ),
        hint: module_names.is_empty().then(|| {
            "SoapySDR loaded no driver modules, so it can reach no hardware of its own. \
             RTL-SDR, HackRF, Airspy, AD936x, SDRplay and CR-8 receivers do not need it and are \
             unaffected."
                .to_string()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_reports_the_core_its_paths_and_the_modules_it_loaded() {
        let check = health(&RuntimeInfo {
            core_version: Some("0.8.1".to_string()),
            library: Some("/opt/homebrew/lib/libSoapySDR.0.8.dylib".to_string()),
            search_paths: vec!["/opt/homebrew/lib/SoapySDR/modules0.8".to_string()],
            modules: vec!["libbladeRFSupport.so".to_string()],
            error: None,
        });
        assert_eq!(check.status, CheckStatus::Ok);
        assert!(check.detail.contains("core: 0.8.1"));
        assert!(check.detail.contains("libSoapySDR.0.8.dylib"));
        assert!(
            check
                .detail
                .contains("/opt/homebrew/lib/SoapySDR/modules0.8")
        );
        assert!(check.detail.contains("libbladeRFSupport.so"));
        assert!(check.hint.is_none());
    }

    #[test]
    fn a_runtime_with_no_modules_warns_without_blaming_the_native_backends() {
        let check = health(&RuntimeInfo {
            core_version: Some("0.8.1".to_string()),
            library: Some("/usr/lib/libSoapySDR.so.0.8".to_string()),
            search_paths: Vec::new(),
            modules: Vec::new(),
            error: None,
        });
        assert_eq!(check.status, CheckStatus::Warn);
        assert!(check.detail.contains("(none)"));
        assert!(check.hint.is_some_and(|hint| hint.contains("RTL-SDR")));
    }

    #[test]
    fn a_host_without_soapysdr_warns_and_says_which_radios_are_unaffected() {
        let check = health(&RuntimeInfo {
            core_version: None,
            library: None,
            search_paths: Vec::new(),
            modules: Vec::new(),
            error: Some("SoapySDR is not installed".to_string()),
        });
        assert_eq!(check.status, CheckStatus::Warn);
        assert!(check.detail.contains("not installed"));
        assert!(check.hint.is_some_and(|hint| hint.contains("bladeRF")));
    }
}
