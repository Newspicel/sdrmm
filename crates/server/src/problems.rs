use std::{collections::BTreeMap, sync::Mutex};

use sdrmm_engine::Engine;
use sdrmm_wire::{NodeProblem, ProblemSource, ServerEvent};

#[derive(Default)]
pub(crate) struct ProblemBoard {
    held: Mutex<BTreeMap<(String, ProblemSource), Vec<String>>>,
}

impl ProblemBoard {
    pub(crate) fn set(
        &self,
        engine: &Engine,
        source: ProblemSource,
        node: &str,
        texts: Vec<String>,
    ) {
        let changed = {
            let mut held = self.lock();
            let key = (node.to_owned(), source);
            let before = held.get(&key).cloned().unwrap_or_default();
            if texts.is_empty() {
                held.remove(&key);
            } else {
                held.insert(key, texts.clone());
            }
            before != texts
        };
        if changed {
            self.announce(engine);
        }
    }

    pub(crate) fn replace(&self, engine: &Engine, source: ProblemSource, problems: &[NodeProblem]) {
        let changed = {
            let mut held = self.lock();
            let mut fresh: BTreeMap<(String, ProblemSource), Vec<String>> = BTreeMap::new();
            for problem in problems.iter().filter(|problem| problem.source == source) {
                fresh
                    .entry((problem.node.clone(), source))
                    .or_default()
                    .push(problem.text.clone());
            }
            let before: BTreeMap<_, _> = held
                .iter()
                .filter(|((_, held_source), _)| *held_source == source)
                .map(|(key, texts)| (key.clone(), texts.clone()))
                .collect();
            held.retain(|(_, held_source), _| *held_source != source);
            let changed = before != fresh;
            held.extend(fresh);
            changed
        };
        if changed {
            self.announce(engine);
        }
    }

    pub(crate) fn retain(&self, engine: &Engine, keep: impl Fn(&str, ProblemSource) -> bool) {
        let changed = {
            let mut held = self.lock();
            let before = held.len();
            held.retain(|(node, source), _| keep(node, *source));
            held.len() != before
        };
        if changed {
            self.announce(engine);
        }
    }

    pub(crate) fn all(&self) -> Vec<NodeProblem> {
        self.lock()
            .iter()
            .flat_map(|((node, source), texts)| {
                texts.iter().map(move |text| NodeProblem {
                    node: node.clone(),
                    source: *source,
                    text: text.clone(),
                })
            })
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn of(&self, node: &str) -> Vec<String> {
        self.lock()
            .iter()
            .filter(|((held, _), _)| held == node)
            .flat_map(|(_, texts)| texts.iter().cloned())
            .collect()
    }

    fn announce(&self, engine: &Engine) {
        engine.emit_event(ServerEvent::NodeProblems {
            problems: self.all(),
        });
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<(String, ProblemSource), Vec<String>>> {
        self.held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problem(node: &str, text: &str) -> NodeProblem {
        NodeProblem {
            node: node.to_owned(),
            source: ProblemSource::Bind,
            text: text.to_owned(),
        }
    }

    fn announced(rx: &mut tokio::sync::broadcast::Receiver<ServerEvent>) -> usize {
        std::iter::from_fn(|| rx.try_recv().ok())
            .filter(|event| matches!(event, ServerEvent::NodeProblems { .. }))
            .count()
    }

    #[test]
    fn problems_are_announced_only_when_they_change() {
        let engine = Engine::new(None);
        let mut rx = engine.subscribe_events();
        let board = ProblemBoard::default();

        board.replace(
            &engine,
            ProblemSource::Bind,
            &[problem("dev", "Radio not open")],
        );
        board.replace(
            &engine,
            ProblemSource::Bind,
            &[problem("dev", "Radio not open")],
        );
        assert_eq!(announced(&mut rx), 1);

        board.set(
            &engine,
            ProblemSource::Monitor,
            "mon",
            vec!["Failed".to_owned()],
        );
        board.set(
            &engine,
            ProblemSource::Monitor,
            "mon",
            vec!["Failed".to_owned()],
        );
        assert_eq!(announced(&mut rx), 1);
        assert_eq!(board.all().len(), 2);

        board.replace(&engine, ProblemSource::Bind, &[]);
        assert_eq!(announced(&mut rx), 1);
        assert_eq!(board.of("mon"), ["Failed"]);
        assert!(board.of("dev").is_empty());
    }

    #[test]
    fn problems_of_nodes_that_left_are_dropped_once() {
        let engine = Engine::new(None);
        let board = ProblemBoard::default();
        board.set(
            &engine,
            ProblemSource::Monitor,
            "mon",
            vec!["Failed".to_owned()],
        );
        board.set(
            &engine,
            ProblemSource::Position,
            "rec",
            vec!["Busy".to_owned()],
        );
        let mut rx = engine.subscribe_events();

        board.retain(&engine, |node, _| node == "rec");
        board.retain(&engine, |node, _| node == "rec");
        assert_eq!(announced(&mut rx), 1);
        assert!(board.of("mon").is_empty());
        assert_eq!(board.of("rec"), ["Busy"]);
    }
}
