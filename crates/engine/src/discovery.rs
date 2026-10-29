use std::time::{Duration, Instant};

use sdrmm_wire::DeviceInfo;

/// How long a network search's result stands before another request may pay for a fresh one.
/// A plugged or unplugged radio expires it early, so this only paces repeated asking.
const SETTLED: Duration = Duration::from_secs(10);

/// What the last network-reaching search found beyond what is attached to this machine.
///
/// A radio answering over the network takes seconds to find, which is too long to make the device
/// list wait: the list is answered from what is attached now plus whatever the last deep search
/// turned up, and the fresh search runs behind it.
#[derive(Debug, Default)]
pub(crate) struct Discovery {
    attached: Vec<DeviceInfo>,
    extras: Vec<DeviceInfo>,
    searching: bool,
    searched: Option<Instant>,
}

impl Discovery {
    pub(crate) fn merge(&self, mut attached: Vec<DeviceInfo>) -> Vec<DeviceInfo> {
        let ids: Vec<String> = attached.iter().map(DeviceInfo::id).collect();
        attached.extend(
            self.extras
                .iter()
                .filter(|extra| !ids.contains(&extra.id()))
                .cloned(),
        );
        attached.sort_by_key(DeviceInfo::id);
        attached
    }

    pub(crate) fn note(&mut self, attached: Vec<DeviceInfo>) {
        self.attached = attached;
    }

    pub(crate) fn seen(&self) -> Vec<DeviceInfo> {
        self.merge(self.attached.clone())
    }

    /// Claims the right to search, so that a burst of requests starts one search and not one each.
    pub(crate) fn claim(&mut self, now: Instant) -> bool {
        if self.searching || self.searched.is_some_and(|at| now < at + SETTLED) {
            return false;
        }
        self.searching = true;
        true
    }

    /// Files what the search found and reports whether the device list changed because of it.
    pub(crate) fn searched(&mut self, extras: Vec<DeviceInfo>, now: Instant) -> bool {
        self.searching = false;
        self.searched = Some(now);
        let changed = self.extras != extras;
        self.extras = extras;
        changed
    }

    /// Lets the next request search again, for when the machine changed under us.
    pub(crate) fn expire(&mut self) {
        self.searched = None;
    }
}

/// What a deep search turned up beyond the radios attached to this machine. The attached listing
/// is read either side of the search because opening a radio takes it out of its driver's
/// listing: a radio either read saw is attached, not a network find that would then outlive the
/// search in the device list.
pub(crate) fn extras_beyond(
    found: Vec<DeviceInfo>,
    before: &[String],
    after: &[String],
) -> Vec<DeviceInfo> {
    found
        .into_iter()
        .filter(|device| {
            let id = device.id();
            !before.contains(&id) && !after.contains(&id)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::DeviceBackend;

    use super::*;

    fn device(driver: DeviceBackend, key: &str) -> DeviceInfo {
        DeviceInfo {
            driver,
            key: key.to_string(),
            label: format!("{driver} {key}"),
            serial: None,
            profile: None,
        }
    }

    fn ids(devices: &[DeviceInfo]) -> Vec<String> {
        devices.iter().map(DeviceInfo::id).collect()
    }

    #[test]
    fn a_network_radio_joins_the_attached_ones() {
        let mut discovery = Discovery::default();
        discovery.searched(vec![device(DeviceBackend::Soapy, "remote")], Instant::now());
        assert_eq!(
            ids(&discovery.merge(vec![device(DeviceBackend::Rtlsdr, "0")])),
            vec!["rtlsdr:0", "soapy:remote"]
        );
    }

    #[test]
    fn a_radio_the_quick_search_already_found_is_listed_once() {
        let mut discovery = Discovery::default();
        discovery.searched(vec![device(DeviceBackend::Soapy, "one")], Instant::now());
        assert_eq!(
            ids(&discovery.merge(vec![device(DeviceBackend::Soapy, "one")])),
            vec!["soapy:one"]
        );
    }

    #[test]
    fn one_search_runs_however_often_the_list_is_asked_for() {
        let now = Instant::now();
        let mut discovery = Discovery::default();
        assert!(discovery.claim(now));
        assert!(!discovery.claim(now), "a search is already running");
        discovery.searched(Vec::new(), now);
        assert!(!discovery.claim(now), "the answer is still fresh");
        assert!(discovery.claim(now + SETTLED));
    }

    #[test]
    fn a_changed_machine_earns_a_fresh_search_at_once() {
        let now = Instant::now();
        let mut discovery = Discovery::default();
        assert!(discovery.claim(now));
        discovery.searched(Vec::new(), now);
        discovery.expire();
        assert!(discovery.claim(now));
    }

    #[test]
    fn a_radio_opened_while_the_search_ran_is_no_network_find() {
        let band = device(DeviceBackend::Virtual, "band");
        let remote = device(DeviceBackend::Soapy, "remote");
        let found = vec![band.clone(), remote];
        assert_eq!(
            ids(&extras_beyond(found.clone(), &[band.id()], &[])),
            vec!["soapy:remote"]
        );
        assert_eq!(
            ids(&extras_beyond(found.clone(), &[], &[band.id()])),
            vec!["soapy:remote"]
        );
        assert_eq!(
            ids(&extras_beyond(found, &[], &[])),
            vec!["virtual:band", "soapy:remote"]
        );
    }

    #[test]
    fn only_a_different_result_counts_as_a_change() {
        let now = Instant::now();
        let mut discovery = Discovery::default();
        assert!(discovery.searched(vec![device(DeviceBackend::Soapy, "remote")], now));
        assert!(!discovery.searched(vec![device(DeviceBackend::Soapy, "remote")], now));
        assert!(discovery.searched(Vec::new(), now));
    }
}
