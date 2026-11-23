use std::time::Duration;

use sdrmm_voicechat::SignedIn;
use sdrmm_wire::{MatrixSignInPrompt, MatrixSignInState};
use tokio::sync::{oneshot, watch};

use super::*;

const BOT: &str = "@bot:matrix.example";
const OTHER: &str = "@other:matrix.example";
const ROOM: &str = "!room:matrix.example";

fn session_of(user: &str) -> String {
    format!(
        r#"{{"kind":"oauth","client_id":"c","user_id":"{user}","device_id":"D","access_token":"t"}}"#
    )
}

fn hub() -> (Arc<Sessions>, Account) {
    let store = Arc::new(Store::open(None).expect("store"));
    let bot = store
        .create_account(Account {
            id: String::new(),
            name: "Bot".to_owned(),
            service: AccountService::Matrix {
                homeserver_url: "https://matrix.example".to_owned(),
                user: BOT.to_owned(),
                login: MatrixLogin::Oauth,
                access_token: Secret::default(),
                password: Secret::default(),
                nickname: String::new(),
                recovery_key: Secret::default(),
                session: Secret::default(),
                store_key: Secret::default(),
            },
        })
        .expect("created");
    (Arc::new(Sessions::new(store, None)), bot)
}

fn prompt() -> MatrixSignInPrompt {
    MatrixSignInPrompt {
        verification_uri: "https://auth.matrix.example/link".to_owned(),
        user_code: "CODE".to_owned(),
        expires_in_s: 600,
    }
}

fn kept_session(sessions: &Sessions, account: &str) -> Option<String> {
    let account = sessions
        .store
        .account(account)
        .expect("read")
        .expect("found");
    let AccountService::Matrix { session, .. } = &account.service else {
        panic!("not matrix");
    };
    sessions.reveal(session, "Session").expect("revealed")
}

fn answered(signed: Result<SignedIn, String>) -> oneshot::Receiver<Result<SignedIn, String>> {
    let (answer, done) = oneshot::channel();
    answer.send(signed).expect("sent");
    done
}

fn approved(user: &str) -> oneshot::Receiver<Result<SignedIn, String>> {
    answered(Ok(SignedIn {
        user: user.to_owned(),
        session: session_of(user),
    }))
}

async fn eventually(holds: impl Fn() -> bool) {
    let waited = tokio::time::timeout(Duration::from_secs(5), async {
        while !holds() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });
    waited.await.expect("never held");
}

#[tokio::test]
async fn an_approved_sign_in_saves_its_session_then_goes_idle() {
    let (sessions, bot) = hub();
    let attempt = sessions.show_sign_in(&bot.id, prompt());
    assert_eq!(
        sessions.sign_in_state(&bot.id),
        MatrixSignInState::Pending(prompt())
    );
    sessions
        .clone()
        .finish_sign_in(bot.id.clone(), attempt, approved(BOT))
        .await;
    assert_eq!(sessions.sign_in_state(&bot.id), MatrixSignInState::Idle);
    assert_eq!(kept_session(&sessions, &bot.id), Some(session_of(BOT)));
}

#[tokio::test]
async fn a_replaced_sign_in_saves_nothing() {
    let (sessions, bot) = hub();
    let first = sessions.show_sign_in(&bot.id, prompt());
    let _second = sessions.show_sign_in(&bot.id, prompt());
    sessions
        .clone()
        .finish_sign_in(bot.id.clone(), first, approved(BOT))
        .await;
    assert_eq!(kept_session(&sessions, &bot.id), None);
    assert_eq!(
        sessions.sign_in_state(&bot.id),
        MatrixSignInState::Pending(prompt())
    );
}

#[tokio::test]
async fn another_users_session_is_not_saved() {
    let (sessions, bot) = hub();
    let attempt = sessions.show_sign_in(&bot.id, prompt());
    sessions
        .clone()
        .finish_sign_in(bot.id.clone(), attempt, approved(OTHER))
        .await;
    assert_eq!(kept_session(&sessions, &bot.id), None);
    assert_eq!(
        sessions.sign_in_state(&bot.id),
        MatrixSignInState::Failed {
            error: NOT_SAVED.to_owned()
        }
    );
}

#[tokio::test]
async fn a_refused_or_abandoned_sign_in_fails() {
    let (sessions, bot) = hub();
    let attempt = sessions.show_sign_in(&bot.id, prompt());
    let refused = answered(Err("Sign-in expired".to_owned()));
    sessions
        .clone()
        .finish_sign_in(bot.id.clone(), attempt, refused)
        .await;
    assert_eq!(
        sessions.sign_in_state(&bot.id),
        MatrixSignInState::Failed {
            error: "Sign-in expired".to_owned()
        }
    );
    let attempt = sessions.show_sign_in(&bot.id, prompt());
    let (_, abandoned) = oneshot::channel();
    sessions
        .clone()
        .finish_sign_in(bot.id.clone(), attempt, abandoned)
        .await;
    assert_eq!(
        sessions.sign_in_state(&bot.id),
        MatrixSignInState::Failed {
            error: "Sign-in failed".to_owned()
        }
    );
}

#[tokio::test]
async fn a_pending_sign_in_holds_the_session_shut() {
    let (sessions, bot) = hub();
    let _link = sessions.join(&bot, ROOM).expect("joined");
    sessions.show_sign_in(&bot.id, prompt());
    assert!(sessions.lock().is_empty());
    assert_eq!(sessions.join(&bot, ROOM).err(), Some(Unjoined::Waiting));
}

#[tokio::test]
async fn the_keeper_saves_each_session_and_a_new_session_clears_its_fault() {
    let (sessions, bot) = hub();
    let (saved, watched) = watch::channel(None);
    let keeper = sessions.keep(&bot.id, None, watched).expect("keeper");
    saved.send_replace(Some(session_of(BOT)));
    eventually(|| kept_session(&sessions, &bot.id) == Some(session_of(BOT))).await;
    saved.send_replace(Some(session_of(OTHER)));
    eventually(|| sessions.fault(&bot.id).as_deref() == Some(NOT_SAVED)).await;
    assert_eq!(kept_session(&sessions, &bot.id), Some(session_of(BOT)));
    keeper.abort();

    let _link = sessions.join(&bot, ROOM).expect("joined");
    assert_eq!(sessions.fault(&bot.id), None);
}
