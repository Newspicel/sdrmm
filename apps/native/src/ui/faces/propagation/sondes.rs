use std::{cell::Cell, time::Duration};

use sdrmm_wire::propagation::IonosondeReport;
use zgui::{
    prelude::*,
    reactive::{ArcRwSignal, RenderEffect},
};

use crate::{
    store::Store,
    ui::kit_maps::{every, now_ms},
};

pub const STALE_MS: i64 = 10 * 60_000;
const REFETCH: Duration = Duration::from_secs(15 * 60);
const PATH: &str = "/api/ionosonde";

pub type Answer = Option<Result<IonosondeReport, String>>;

struct Cache {
    answer: ArcRwSignal<Answer>,
    fetched_at: Cell<Option<i64>>,
    asking: Cell<bool>,
}

thread_local! {
    static CACHE: Cache = Cache {
        answer: ArcRwSignal::new(None),
        fetched_at: Cell::new(None),
        asking: Cell::new(false),
    };
}

#[must_use]
pub fn is_fresh(fetched_at: Option<i64>, now: i64) -> bool {
    fetched_at.is_some_and(|at| now - at < STALE_MS)
}

#[must_use]
pub fn answer() -> ArcRwSignal<Answer> {
    CACHE.with(|cache| cache.answer.clone())
}

fn ask(store: Store) {
    let wanted = CACHE.with(|cache| {
        let wanted = !cache.asking.get() && !is_fresh(cache.fetched_at.get(), now_ms());
        if wanted {
            cache.asking.set(true);
        }
        wanted
    });
    if !wanted {
        return;
    }
    zgui::task::spawn_local(async move {
        let answer = store.api().get::<IonosondeReport>(PATH).await;
        CACHE.with(|cache| {
            cache.asking.set(false);
            cache.fetched_at.set(Some(now_ms()));
            cache
                .answer
                .set(Some(answer.map_err(|error| error.to_string())));
        });
    });
}

pub fn follow(store: Store, enabled: Memo<bool>) {
    let asking = RenderEffect::new(move |_| {
        if enabled.get() {
            ask(store);
        }
    });
    on_cleanup_local(move || drop(asking));
    every(REFETCH, move || {
        if enabled.get_untracked() {
            ask(store);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_is_reused_for_ten_minutes() {
        assert!(!is_fresh(None, 0));
        assert!(is_fresh(Some(1_000), 1_000 + STALE_MS - 1));
        assert!(!is_fresh(Some(1_000), 1_000 + STALE_MS));
    }
}
