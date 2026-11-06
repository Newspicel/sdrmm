use std::{
    io,
    path::{Path, PathBuf},
};

use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use sdrmm_server::StoreError;

use crate::{exit::Exit, update};

const UPDATE: &str = "Update";
const DELETE: &str = "Delete database";
const QUIT: &str = "Quit";

#[derive(Debug, PartialEq, Eq)]
enum Choice {
    Update,
    Delete,
    Quit,
}

pub fn ask(db: PathBuf, error: &StoreError) -> Exit {
    let answer = MessageDialog::new()
        .set_title("Database is newer")
        .set_description(format!("{error}\n\n{}", db.display()))
        .set_level(MessageLevel::Warning)
        .set_buttons(MessageButtons::YesNoCancelCustom(
            UPDATE.to_owned(),
            DELETE.to_owned(),
            QUIT.to_owned(),
        ))
        .show();
    match choice(&answer) {
        Choice::Update => update::update_now(),
        Choice::Delete => match delete(&db) {
            Ok(()) => Exit::Relaunch,
            Err(e) => {
                tracing::error!("could not delete {}: {e}", db.display());
                Exit::Failed
            }
        },
        Choice::Quit => Exit::Quit,
    }
}

fn choice(answer: &MessageDialogResult) -> Choice {
    match answer {
        MessageDialogResult::Custom(label) if label == UPDATE => Choice::Update,
        MessageDialogResult::Custom(label) if label == DELETE => Choice::Delete,
        MessageDialogResult::Yes => Choice::Update,
        MessageDialogResult::No => Choice::Delete,
        _ => Choice::Quit,
    }
}

fn delete(db: &Path) -> io::Result<()> {
    for suffix in ["", "-wal", "-shm"] {
        let mut file = db.as_os_str().to_owned();
        file.push(suffix);
        match std::fs::remove_file(&file) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_removes_the_database_and_its_journal() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("sdrmm.db");
        std::fs::write(&db, b"db").unwrap();
        std::fs::write(dir.path().join("sdrmm.db-wal"), b"wal").unwrap();
        delete(&db).unwrap();
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn every_platform_answer_maps_to_one_choice() {
        let custom = |label: &str| MessageDialogResult::Custom(label.to_owned());
        assert_eq!(choice(&custom(UPDATE)), Choice::Update);
        assert_eq!(choice(&custom(DELETE)), Choice::Delete);
        assert_eq!(choice(&custom(QUIT)), Choice::Quit);
        assert_eq!(choice(&MessageDialogResult::Yes), Choice::Update);
        assert_eq!(choice(&MessageDialogResult::No), Choice::Delete);
        assert_eq!(choice(&MessageDialogResult::Cancel), Choice::Quit);
    }
}
