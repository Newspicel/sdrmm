use rfd::{AsyncMessageDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};

pub async fn tell(message: String) {
    AsyncMessageDialog::new()
        .set_title("SDRmm")
        .set_description(message)
        .set_level(MessageLevel::Error)
        .set_buttons(MessageButtons::Ok)
        .show()
        .await;
}

pub async fn confirm(title: &str, message: String, yes: &str, no: &str) -> bool {
    let choice = AsyncMessageDialog::new()
        .set_title(title)
        .set_description(message)
        .set_buttons(MessageButtons::OkCancelCustom(
            yes.to_owned(),
            no.to_owned(),
        ))
        .show()
        .await;
    chose(&choice, yes)
}

pub fn inform(title: &str, message: &str) {
    MessageDialog::new()
        .set_title(title)
        .set_description(message)
        .set_buttons(MessageButtons::Ok)
        .show();
}

pub fn fail(message: &str) {
    MessageDialog::new()
        .set_title("SDRmm")
        .set_description(message)
        .set_level(MessageLevel::Error)
        .set_buttons(MessageButtons::Ok)
        .show();
}

/// Platforms report a custom button either by its label or, for the first one, as `Ok`/`Yes`.
pub fn chose(choice: &MessageDialogResult, label: &str) -> bool {
    match choice {
        MessageDialogResult::Custom(chosen) => chosen == label,
        MessageDialogResult::Ok | MessageDialogResult::Yes => true,
        MessageDialogResult::No | MessageDialogResult::Cancel => false,
    }
}
