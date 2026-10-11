use std::collections::HashSet;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ApplyGate {
    #[default]
    Idle,
    Running,
    Again,
}

impl ApplyGate {
    pub fn ask(&mut self) -> bool {
        let start = *self == Self::Idle;
        *self = if start { Self::Running } else { Self::Again };
        start
    }

    pub fn finished(&mut self) -> bool {
        let again = *self == Self::Again;
        *self = if again { Self::Running } else { Self::Idle };
        again
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Switches {
    expected: HashSet<i64>,
}

impl Switches {
    pub fn expect(&mut self, id: i64) {
        self.expected.insert(id);
    }

    pub fn arrived(&mut self, id: i64) -> bool {
        self.expected.remove(&id)
    }
}

#[must_use]
pub fn switch_notice(by: Option<&str>, name: Option<&str>) -> String {
    format!(
        "{} switched to {}",
        by.unwrap_or("Someone"),
        name.unwrap_or("another workspace")
    )
}

#[must_use]
pub fn gone_nodes<'a>(
    held: impl IntoIterator<Item = &'a String>,
    present: &HashSet<&str>,
) -> Vec<String> {
    held.into_iter()
        .filter(|id| !present.contains(id.as_str()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_apply_asked_while_one_runs_runs_once_more() {
        let mut gate = ApplyGate::default();
        assert!(gate.ask());
        assert!(!gate.ask());
        assert!(!gate.ask());
        assert!(gate.finished());
        assert!(!gate.finished());
        assert_eq!(gate, ApplyGate::Idle);
    }

    #[test]
    fn my_own_switch_is_not_announced() {
        let mut switches = Switches::default();
        switches.expect(4);
        assert!(switches.arrived(4));
        assert!(!switches.arrived(4));
        assert!(!switches.arrived(5));
    }

    #[test]
    fn a_switch_names_who_and_where() {
        assert_eq!(
            switch_notice(Some("Ann"), Some("Lab")),
            "Ann switched to Lab"
        );
        assert_eq!(
            switch_notice(None, None),
            "Someone switched to another workspace"
        );
    }

    #[test]
    fn nodes_no_longer_drawn_are_gone() {
        let held = ["a".to_owned(), "b".to_owned()];
        let present: HashSet<&str> = ["a"].into_iter().collect();
        assert_eq!(gone_nodes(&held, &present), ["b".to_owned()]);
    }
}
