use std::{
    process::{Command, ExitCode},
    sync::atomic::{AtomicBool, Ordering},
};

static RELAUNCH: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    Quit,
    Relaunch,
    Failed,
}

impl Exit {
    /// What the window asked for when it closed.
    pub fn requested() -> Self {
        if RELAUNCH.load(Ordering::Relaxed) {
            Self::Relaunch
        } else {
            Self::Quit
        }
    }

    pub fn finish(self) -> ExitCode {
        match self {
            Self::Quit => ExitCode::SUCCESS,
            Self::Failed => ExitCode::FAILURE,
            Self::Relaunch => relaunch(),
        }
    }
}

pub fn relaunch_on_close() {
    RELAUNCH.store(true, Ordering::Relaxed);
}

fn relaunch() -> ExitCode {
    let started = std::env::current_exe().and_then(|exe| Command::new(exe).spawn());
    match started {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "could not start SDRmm again");
            ExitCode::FAILURE
        }
    }
}
