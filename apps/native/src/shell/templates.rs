use sdrmm_wire::{rest::TemplateInfo, state::DeviceSet};

#[must_use]
pub fn supports(template: &TemplateInfo, set: Option<&DeviceSet>) -> bool {
    set.is_some_and(|set| template.supported_devices.contains(&set.device.id()))
}

#[must_use]
pub fn templates_hint(templates: &[TemplateInfo], set: Option<&DeviceSet>) -> Option<String> {
    let Some(set) = set else {
        return Some(String::from("Select a device first."));
    };
    (!templates.is_empty()
        && !templates
            .iter()
            .any(|template| supports(template, Some(set))))
    .then(|| format!("{} cannot run these templates.", set.device.label))
}

#[must_use]
pub fn template_size(template: &TemplateInfo) -> String {
    if template.min_lanes > 1 {
        format!("{} lanes", template.min_lanes)
    } else {
        format!("{} ch", template.channels.len())
    }
}

#[must_use]
pub fn apply_title(template: &TemplateInfo, set: Option<&DeviceSet>) -> Option<String> {
    let set = set?;
    Some(if supports(template, Some(set)) {
        format!("Apply to {}", set.device.label)
    } else {
        format!("{} cannot run this template", set.device.label)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(supported: &[&str]) -> TemplateInfo {
        serde_json::from_value(serde_json::json!({
            "id": "airband",
            "name": "Airband",
            "description": "",
            "explainer": "",
            "center_hz": 125e6,
            "sample_rate": 2.4e6,
            "channels": [],
            "min_freq_hz": 118e6,
            "max_freq_hz": 137e6,
            "supported_devices": supported,
        }))
        .expect("a template")
    }

    fn set() -> DeviceSet {
        serde_json::from_value(serde_json::json!({
            "id": 1,
            "device": { "driver": "rtlsdr", "key": "0", "label": "RTL-SDR" },
            "capabilities": {
                "freq_ranges": [], "sample_rates": [], "gains": [],
                "antennas": [], "bandwidths": [], "duplex": "rx_only"
            },
            "settings": {},
            "status": "running",
            "channels": [],
        }))
        .expect("a device set")
    }

    #[test]
    fn a_template_runs_on_the_devices_it_names() {
        assert!(supports(&template(&["rtlsdr:0"]), Some(&set())));
        assert!(!supports(&template(&["hackrf:0"]), Some(&set())));
        assert!(!supports(&template(&["rtlsdr:0"]), None));
    }

    #[test]
    fn hints_at_a_missing_or_unfit_device() {
        assert_eq!(
            templates_hint(&[template(&[])], None).as_deref(),
            Some("Select a device first.")
        );
        assert_eq!(
            templates_hint(&[template(&["hackrf:0"])], Some(&set())).as_deref(),
            Some("RTL-SDR cannot run these templates.")
        );
        assert_eq!(
            templates_hint(&[template(&["rtlsdr:0"])], Some(&set())),
            None
        );
        assert_eq!(templates_hint(&[], Some(&set())), None);
    }

    #[test]
    fn sizes_a_template_by_lanes_or_channels() {
        assert_eq!(template_size(&template(&[])), "0 ch");
        let mut array = template(&[]);
        array.min_lanes = 4;
        assert_eq!(template_size(&array), "4 lanes");
    }

    #[test]
    fn titles_apply_with_the_radio_it_lands_on() {
        assert_eq!(apply_title(&template(&["rtlsdr:0"]), None), None);
        assert_eq!(
            apply_title(&template(&["rtlsdr:0"]), Some(&set())).as_deref(),
            Some("Apply to RTL-SDR")
        );
        assert_eq!(
            apply_title(&template(&["hackrf:0"]), Some(&set())).as_deref(),
            Some("RTL-SDR cannot run this template")
        );
    }
}
