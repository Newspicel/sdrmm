# Matrix Voice Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Matrix as a third chat service: a Voice chat node joins a room's MatrixRTC call over LiveKit and trades audio both ways, E2EE included.

**Architecture:** `sdrmm-voicechat` gains `matrix`: matrix-sdk (git main) for login, sync, membership and Olm keys; an own LiveKit signal client on `livekit-protocol` over `tokio-tungstenite`; `str0m` (rust-crypto) for WebRTC; LiveKit's frame cryptor rewritten on RustCrypto. The server stores the account, its saved session and store key as secrets, and runs the OAuth device sign-in.

**Tech Stack:** Rust (tokio, matrix-sdk git main + matrix-sdk-sqlite, livekit-protocol 0.9, str0m 0.26, tokio-tungstenite 0.30, reqwest 0.13, aes-gcm, hkdf, sha2, base64), React/TypeScript web.

**Spec:** `docs/superpowers/specs/2026-11-23-matrix-voice-design.md`

## Global Constraints
- AGENTS.md: no `//` comments, `///` only for rationale; no `unwrap`/`expect` outside tests and startup; functions under 200 lines, files under 3000; short UI text, no em dash; one control language (dropdowns, fields, sliders).
- `sdrmm-voicechat` keeps no internal deps (`xtask/src/architecture.rs` unchanged) and stays off phones.
- No C/C++ media stack: never the `livekit` crate or webrtc-sys. `str0m` with `default-features = false, features = ["rust-crypto"]`.
- `matrix-sdk` and `matrix-sdk-sqlite` pinned by `git = "https://github.com/matrix-org/matrix-rust-sdk", rev = "<main HEAD sha>"`, with a `#` comment in `Cargo.toml` saying to return to crates.io once a release uses rusqlite 0.41. The workspace `rusqlite` 0.41 stays. `cargo deny check` must pass.
- Every new secret (`password`, `recovery_key`, `session`, `store_key`) is a `Secret`; `Debug` of `Service` prints none of them.
- A frame that cannot be decrypted or decoded increments `Link::drops`; a failed join or login is `LinkState::Failed(short text)`.
- Work happens on the `worktree-matrix-voice` branch in its worktree; commit each task there. Never touch `main`, never push.
- Finish each task with `cargo fmt`, `cargo clippy -p <crate> --all-targets -- -D warnings`, and the tests of what changed.

## Review Focus
- Token refresh while in a call: the saved session changes but the link stays joined (Task 9 test `a_refreshed_session_keeps_the_link`).
- A participant joins before their key arrives: frames count as drops until it does, then audio flows (Task 8 test `audio_flows_once_the_key_arrives`).
- The bot's own frames echoed by the SFU or its own membership: never heard as a talker (Task 4 test `our_own_membership_is_not_a_peer`).
- An unencrypted room: no key exchange, frames pass in clear, publish with `Encryption::None` (Task 8 test `a_clear_room_sends_frames_as_they_are`).
- Account edited from OAuth to password or to another user: the saved session is dropped and the store folder starts over (Task 10 test `another_user_forgets_the_session`).

---

### Task 1: Dependencies
**Files:** Modify `Cargo.toml` (workspace deps), `crates/voicechat/Cargo.toml`, `Cargo.lock`, `deny.toml` only if a new licence needs allowing (report it, do not add copyleft-incompatible ones).

- [ ] Find main HEAD: `git ls-remote https://github.com/matrix-org/matrix-rust-sdk refs/heads/main`.
- [ ] Workspace: `matrix-sdk = { git, rev, default-features = false, default-features = false` with the features main names for E2EE, SQLite, rustls, OAuth and the ruma unstable flags for MSC4140 and MSC4143; `livekit-protocol = "0.9"`, `str0m`, `aes-gcm`, `hkdf`, `base64` at their newest stable; reuse `reqwest`, `tokio-tungstenite`, `sha2`, `url`, `serde_json`, `futures`. `prost` at the version `livekit-protocol` uses.
- [ ] `cargo check -p sdrmm-voicechat` and `cargo tree -i libsqlite3-sys` shows one version. `cargo deny check` passes.

### Task 2: Frame cryptor
**Files:** Create `crates/voicechat/src/matrix/e2ee/frame.rs` (tests inline). Module wiring in `crates/voicechat/src/matrix.rs` and `matrix/e2ee.rs`.

**Produces:**
- `pub(crate) const KEY_RING: usize = 256;`
- `pub(crate) struct FrameKey(aes_gcm::Aes128Gcm)`; `FrameKey::derive(material: &[u8]) -> Result<Self, CryptoError>`: HKDF-SHA256, salt `b"LKFrameEncryptionKey"`, info 128 zero bytes, 16-byte output. Any material length is accepted.
- `pub(crate) struct KeyRing { keys: Box<[Option<FrameKey>; KEY_RING]> }` with `set(&mut self, index: u8, key: FrameKey)`, `get(&self, index: u8) -> Option<&FrameKey>`.
- `pub(crate) struct Sealer { ssrc: u32, sent: u32 }`; `seal(&mut self, key: &FrameKey, index: u8, timestamp: u32, opus: &[u8]) -> Result<Vec<u8>, CryptoError>`. Output: `opus[0]` clear, then AES-GCM ciphertext of `opus[1..]` with AAD `opus[..1]` (tag included), then the 12-byte IV, then `[12, index]`. IV: `be32(ssrc) ‖ be32(timestamp) ‖ be32(timestamp.wrapping_sub(sent % 0xffff))`, `sent` counts frames.
- `open(ring: &KeyRing, frame: &[u8]) -> Result<Vec<u8>, CryptoError>`: reads trailer, IV and index; empty or too short is an error.
- `#[derive(thiserror::Error)] enum CryptoError { Short, NoKey(u8), Auth }`.

- [ ] Tests: `a_frame_round_trips`, `the_first_byte_stays_clear`, `a_wrong_key_fails`, `a_missing_index_names_it`, `a_short_frame_is_refused`, `livekit_vector_opens`: a frame and 16-byte key produced by `scripts/livekit-frame-vector.mjs` (new, Node WebCrypto with the same constants, cites livekit client-sdk-js `src/e2ee/constants.ts` and `worker/FrameCryptor.ts` in its header) opens to the expected bytes.
- [ ] Implement, `cargo test -p sdrmm-voicechat matrix::e2ee::frame`.

### Task 3: Media keys
**Files:** Create `crates/voicechat/src/matrix/e2ee/keys.rs` (tests inline).

**Interfaces:** Consumes Task 2 `FrameKey`, `KeyRing`.

**Produces:**
- `EncryptionKeys` serde content of to-device `io.element.call.encryption_keys` (field names as matrix-js-sdk `src/matrixrtc/ToDeviceKeyTransport.ts` sends today; check its source): `keys: { index: u8, key: String /* unpadded base64 */ }`, `member: { id: String, claimed_device_id: String }`, `room_id: String`, `session: { application: "m.call", call_id: "", scope: "m.room" }`, `sent_ts: u64`. Also accept the stable `m.rtc.encryption_key` type if main ruma or js-sdk defines it.
- `MediaKeys` (pure, clock passed in): `new(now: Instant) -> Self` makes a random 16-byte key at index 0; `ours(&self, now: Instant) -> (u8, &FrameKey)` the key in use; `on_left(&mut self, now) -> Outgoing` new key at next index, used from `now + 5 s`; `on_joined(&self) -> Outgoing` current key; `receive(&mut self, from: &str /* participant identity */, index: u8, material: &[u8])`; `ring(&self, identity: &str) -> Option<&KeyRing>`. `Outgoing { index: u8, material: [u8; 16] }`.

- [ ] Tests: `a_js_sdk_key_event_parses` (fixture JSON), `leaving_rotates_after_grace`, `joining_resends_the_current_key`, `received_keys_land_in_the_senders_ring`, `index_wraps_after_255`.
- [ ] Implement, `cargo test -p sdrmm-voicechat matrix::e2ee`.

### Task 4: Call membership
**Files:** Create `crates/voicechat/src/matrix/membership.rs` (tests in `matrix/membership/tests.rs`, fixtures inline).

**Produces:**
- `Focus { service_url: String }` from `GET /_matrix/client/v1/rtc/transports` (MSC4143) or `.well-known` `org.matrix.msc4143.rtc_foci` entries of `type: "livekit"`: `fn foci_from_transports(json) -> Vec<Focus>`, `fn foci_from_well_known(json) -> Vec<Focus>`.
- `Member { user: String, device: String, membership_id: String, focus: Option<Focus> }`; `fn members(states: &[(String /*type*/, String /*state key*/, serde_json::Value)], now_ms: u64) -> Vec<Member>` reading `org.matrix.msc3401.call.member` session form and `m.rtc.member`, skipping empty content and expired ones.
- `fn our_membership(user, device, focus, now_ms) -> (event_type, state_key, content)` in the form current matrix-js-sdk `MembershipManager` sends (check its source at implementation; at plan time that is `org.matrix.msc3401.call.member`, state key `_{user}_{device}_m.call` when the server supports MSC3757 else `{user}_{device}_m.call`, content `application: "m.call"`, `call_id: ""`, `scope: "m.room"`, `device_id`, `expires: 14_400_000`, `focus_active: {type: "livekit", focus_selection: "oldest_membership"}`, `foci_preferred: [{type: "livekit", livekit_service_url, livekit_alias: room_id}]`). Leaving sends `{}` to the same key.
- `fn identities(member: &Member) -> Vec<String>`: every LiveKit identity `lk-jwt-service` may give that member (`{user}:{device}` and the hashed form its current source uses).
- `fn active_focus(members: &[Member], ours: &Focus) -> Focus`: oldest membership's focus, else ours.

- [ ] Tests: `transports_list_livekit_foci`, `well_known_foci_parse`, `legacy_and_new_members_both_read`, `expired_and_empty_members_are_gone`, `our_own_membership_is_not_a_peer` (`members` filtered by a `skip: (&str, &str)` user+device argument), `identities_cover_both_forms`, `oldest_member_picks_the_focus`.
- [ ] Implement, `cargo test -p sdrmm-voicechat matrix::membership`.

### Task 5: LiveKit token
**Files:** Create `crates/voicechat/src/matrix/focus.rs` (tests inline, fake server with `axum` as dev-dep).

**Produces:** `async fn livekit_token(http: &reqwest::Client, focus: &Focus, room: &str, device: &str, openid: &OpenIdToken) -> Result<LiveKitGrant, String>`. Tries `POST {service_url}/get_token` (body as current `lk-jwt-service` README), then `POST {service_url}/sfu/get` with `{room, openid_token: {access_token, token_type, matrix_server_name, expires_in}, device_id}`. `LiveKitGrant { url: String, jwt: String }`. `OpenIdToken` is ruma's.

- [ ] Tests: `new_endpoint_is_tried_first`, `old_endpoint_answers_when_new_is_missing` (404 on `/get_token`), `a_refusal_reads_short` (403 gives "Call server refused").
- [ ] Implement, test.

### Task 6: LiveKit signalling
**Files:** Create `crates/voicechat/src/matrix/livekit.rs`, `matrix/livekit/signal.rs` (tests in `matrix/livekit/signal/tests.rs` with a scripted `tokio-tungstenite` server).

**Produces:**
- `fn rtc_url(grant: &LiveKitGrant) -> Result<url::Url, String>`: `{ws(s) of url}/rtc?access_token=..&auto_subscribe=1&sdk=rust&protocol=<the PROTOCOL_VERSION livekit-api's signal client sends>&adaptive_stream=0`.
- `struct Signal` over one socket, binary protobuf `livekit_protocol::SignalRequest`/`SignalResponse`: `async fn connect(url) -> Result<(Signal, JoinResponse), String>`, `send(&mut self, request: signal_request::Message)`, `async fn next(&mut self) -> Result<signal_response::Message, String>`, and pings every `join.ping_interval` s (fails after `ping_timeout` without pong).
- Helpers: `fn add_mic(encrypted: bool) -> AddTrackRequest` (cid "mic", name "microphone", `TrackType::Audio`, `TrackSource::Microphone`, `Encryption::Gcm` or `None`); `fn leave() -> LeaveRequest` (client initiated, no reconnect); trickle to/from `str0m` candidate JSON with `SignalTarget`.
- `struct Roster`: `apply(&mut self, update: &ParticipantUpdate)`, `identity_of_track(&self, track_sid: &str) -> Option<&str>`, `left(&self) -> Vec<String>`, `joined(&self) -> Vec<String>` since last call.

- [ ] Tests: `join_reads_the_join_response`, `pings_go_out_and_a_silent_server_fails` (tokio test-util time), `roster_maps_tracks_to_identities`, `roster_reports_joins_and_leaves`, `url_switches_https_to_wss`.
- [ ] Implement, test.

### Task 7: WebRTC peers
**Files:** Create `crates/voicechat/src/matrix/livekit/peer.rs` (tests in `peer/tests.rs`).

**Produces:**
- `struct Peer { rtc: str0m::Rtc, socket: tokio::net::UdpSocket, ... }`; `Peer::publisher(ice_servers) -> Result<(Peer, SdpOffer), String>` with one send-only Opus audio media and the `_reliable` and `_lossy` data channels LiveKit clients open; `Peer::subscriber(offer) -> Result<(Peer, SdpAnswer), String>`; `accept_answer`, `add_remote_candidate`, `local_candidates() -> Vec<Candidate>`.
- `async fn run(&mut self, outgoing: &mut mpsc::Receiver<OpusOut>, incoming: &mpsc::Sender<OpusIn>) -> Result<(), String>` drives str0m (poll_output, timeouts, socket I/O) until closed. `OpusOut { payload: Vec<u8>, timestamp: u32 }`, `OpusIn { mid: Mid, track: Option<String> /* from msid */, payload: Vec<u8>, timestamp: u32 }`.
- Local candidate: the UDP socket's address toward the SFU host (`connect` trick on a probe socket). ICE/TCP only when the join response offers no UDP route; leave a `LinkState::Failed("No UDP route")` otherwise.

- [ ] Tests: `two_peers_trade_opus_on_loopback` (a publisher and a str0m answerer in-process over 127.0.0.1), `subscriber_answers_an_offer_with_audio`, `the_track_sid_comes_from_msid`.
- [ ] Implement, test.

### Task 8: Call pipeline
**Files:** Create `crates/voicechat/src/matrix/call.rs` (tests in `call/tests.rs`).

**Interfaces:** Consumes Tasks 2 to 7 and `link::Ends`, `opus::{Encoder, Decoder, Framer}`.

**Produces:**
- `struct Media { keys: Option<MediaKeys>, sealer: Sealer, encoder: Encoder, decoders: HashMap<String, Decoder> }`: `fn outgoing(&mut self, frame: &[f32], timestamp: u32, now) -> Result<Vec<u8>, String>` (Opus, then seal when encrypted); `fn incoming(&mut self, identity: &str, payload: &[u8]) -> Option<Vec<f32>>` (open, decode; a failure calls `Inbox::lose` with "undecryptable" or "undecodable" and answers None).
- `async fn run_call(ctx: CallContext, ends: Ends) -> Ended`: focus → token → membership (with MSC4140 delayed leave of 8 s restarted every 5 s when `/versions` lists `org.matrix.msc4140`, else `expires`) → signal → publisher + subscriber peers → loop over outbox, peers, signal, sync events (membership changes feed `MediaKeys::on_joined/on_left` and to-device sends; key events feed `receive`). On drop of hold or heard: leave signal, send `{}` membership, cancel delayed event. Talker for an identity: `Talker { id: user, name: room display name, verified }`, verified only when `identities(member)` contains it and, in an encrypted room, its key came by Olm from that member's device.

- [ ] Tests (no network, fakes for signal and peers through a small `trait Wire` the loop uses): `a_clear_room_sends_frames_as_they_are`, `an_encrypted_frame_opens_on_the_far_side`, `audio_flows_once_the_key_arrives` (drops counted before), `an_unknown_identity_is_unverified`, `leaving_sends_an_empty_membership`.
- [ ] Implement, test.

### Task 9: Account session and public API
**Files:** Create `crates/voicechat/src/matrix/account.rs`, `matrix/sign_in.rs`; modify `crates/voicechat/src/matrix.rs`, `crates/voicechat/src/lib.rs`. Tests in `matrix/tests.rs` using matrix-sdk's test mocks (`MatrixMockServer` with its testing feature as dev-dep) and in `lib.rs` tests.

**Produces (public, in `lib.rs`):**
- `Service::Matrix(MatrixAccount)`; `pub struct MatrixAccount { pub homeserver: String, pub user: String, pub login: MatrixLogin, pub nickname: String, pub recovery_key: Option<String>, pub store: MatrixStore, pub session: Option<String> }`; `pub enum MatrixLogin { Password(String), OAuth }`; `pub struct MatrixStore { pub dir: Option<PathBuf>, pub passphrase: String }`. `Debug` hides password, recovery key, passphrase, session.
- `renamed_to`: same homeserver, user, login, recovery key and store; `session` Some to Some (a refresh) keeps the session, None to Some reopens it.
- `Session::saved(&self) -> Option<watch::Receiver<Option<String>>>`: the latest session JSON (matrix-sdk `MatrixSession` or `OAuthSession` serialized with a `kind` tag), Some only for Matrix.
- `pub async fn matrix_sign_in(homeserver: &str, store: &MatrixStore, device_name: &str) -> Result<SignIn, VoiceChatError>`; `pub struct SignIn { pub verification_uri: String, pub user_code: String, pub expires_in: Duration, pub done: oneshot::Receiver<Result<SignedIn, String>> }`; `pub struct SignedIn { pub user: String, pub session: String }`. RFC 8628 device grant through matrix-sdk's OAuth API on main if it has it, else `reqwest` against `/_matrix/client/v1/auth_metadata` with dynamic client registration (MSC2966), then `restore_session`.
- `Session::join` for Matrix: room id or alias; one link per room per account (`VoiceChatError::Clash(room)`); empty is `NoChannel`; anything not `!` or `#` is `BadChannel`.

**Inside `account.rs`:** client on the SQLite store at `store.dir` (in-memory when None) with `store.passphrase`; restore `session` or log in with the password (device name "sdrmm"); push every `SessionChange::TokensRefreshed` and first login to `saved`; cross-signing: none on the server → `bootstrap_cross_signing` (password UIA, or none for OAuth), present → `recovery().recover(key)` then verify own device, no key → `Failed("Recovery key needed")`; one `sync` loop per session fanning to-device and state events to calls; rename sets the display name only when it differs and the nickname is not empty. Faults: "Not signed in", "Login refused", "Recovery key needed", "Room not found", "No call server".

- [ ] Tests: `matrix_accounts_print_without_secrets`, `a_refreshed_session_keeps_the_link`, `a_first_session_reopens`, `second_link_in_a_room_clashes`, `a_room_must_be_an_id_or_alias`, `password_login_saves_a_session` (mock server), `restore_skips_login` (mock server, no login request), `oauth_without_session_fails_not_signed_in`, `sign_in_reports_the_code` (mock auth metadata and device endpoint).
- [ ] Implement, `cargo test -p sdrmm-voicechat`, `cargo xtask` architecture check still passes.

### Task 10: Wire, store, hub and REST
**Files:** Modify `crates/wire/src/voice_chat.rs` (+ `voice_chat/tests.rs`), `crates/server/src/store/chat_accounts.rs`, `crates/server/src/voice_chat/hub.rs`, `crates/server/src/voice_chat.rs` (pass `data_dir`), `crates/server/src/rest/chat_accounts.rs`, `openapi.json`, `web/src/generated/schema.d.ts` (`cargo xtask codegen`).

**Produces:**
- Wire: `ChatService::Matrix { homeserver: String, user: String, #[serde(default)] login: MatrixLogin, #[serde(default)] nickname: String, #[serde(default)] recovery_key: Secret, #[serde(default)] session: Secret, #[serde(default)] store_key: Secret }`; `#[serde(tag = "method", rename_all = "snake_case")] enum MatrixLogin { Password { #[serde(default)] password: Secret }, Oauth }`, default Password; `MAX_MATRIX_ID_LEN = 255` for homeserver and user. `MatrixSignInPrompt { verification_uri: String, user_code: String, expires_in_s: u32 }`; `#[serde(tag = "state")] enum MatrixSignInState { Idle, Pending(MatrixSignInPrompt), Failed { error: String } }`.
- Store: `check` requires homeserver and `user` shaped `@local:server`; `session` and `store_key` never come from a request (kept from current; `store_key` made on create as `lower(hex(randomblob(32)))`); `session` is dropped when homeserver, user or login method change; `set_chat_account_session(&self, id: &str, session: &str) -> Result<(), StoreError>`.
- Hub: `Sessions::new(store, data_dir: Option<PathBuf>)`; Matrix service built with `MatrixStore { dir: data_dir.map(|d| d.join("matrix").join(id)), passphrase: store_key }`; on opening a Matrix session spawn a task writing each `saved()` value via `set_chat_account_session` (errors logged with `tracing::error!` and shown as the node's fault).
- REST: `POST /api/chat-accounts/{id}/sign-in` → `MatrixSignInPrompt` (409 when not a Matrix OAuth account), runs `matrix_sign_in` and saves `SignedIn.session` (a different `user` than the account's fails "Signed in as another user"); `GET /api/chat-accounts/{id}/sign-in` → `MatrixSignInState`. `DELETE` also removes `data_dir/matrix/{id}` and answers an error if that fails.

- [ ] Tests: wire `matrix_account_round_trips`, `matrix_login_defaults_to_password`; store `a_request_cannot_set_the_session`, `another_user_forgets_the_session`, `a_store_key_is_made_on_create`, `deleting_removes_every_matrix_secret`; REST `sign_in_refuses_a_password_account`, `deleting_removes_the_store_folder`.
- [ ] Implement, `cargo test -p sdrmm-wire voice_chat`, `cargo test -p sdrmm-server chat_account voice_chat`, `cargo xtask codegen`.

### Task 11: Accounts panel and node
**Files:** Modify `web/src/canvas/nodes/voiceChat.ts` (+ test), `web/src/components/AccountsPanel.tsx` (+ `AccountsPanel.test.tsx`), the API client file the panel uses for chat accounts.

**Produces:** `SERVICE_LABEL.matrix = "Matrix"`, `SERVICES` gains `{ value: "matrix", label: "Matrix", title: "Matrix bot account" }`; `newChatAccount("matrix")` → `{ id: "", name: "", service: "matrix", homeserver: "", user: "", login: { method: "password" }, nickname: "" }`; `channelTitle` for Matrix: "Room id or alias". Form fields: Homeserver, User, Login (dropdown Password | OAuth), Password (password login), Nickname, Recovery key. OAuth: a "Sign in" button; while pending, the link and code show and the panel polls `GET .../sign-in` every 2 s; summary line "Matrix · {user}" and "signed in" or "not signed in" from `session`.

- [ ] Tests: `new matrix account has a password login`, `matrix form shows oauth sign in code`, `channel title names rooms for matrix`.
- [ ] Implement, `cd web && npm run typecheck && npm run lint && npx vitest run src/components/AccountsPanel src/canvas/nodes/voiceChat`.

### Task 12: Docs, changeset, live check
**Files:** Modify `docs/operate/voice-chat.md` (Matrix: bot account, homeserver with MatrixRTC and lk-jwt-service, password or OAuth sign-in, recovery key, room id or alias); `THIRD_PARTY_NOTICES.md` if the project lists deps there. Create `crates/voicechat/tests/matrix_live.rs` (`#[ignore]`, env `SDRMM_MATRIX_HOMESERVER`, `SDRMM_MATRIX_USER`, `SDRMM_MATRIX_PASSWORD`, `SDRMM_MATRIX_ROOM`: joins, reaches `Joined` within 20 s, sends 3 s of 1 kHz).

- [ ] `cargo xtask changeset minor "Voice chat joins Matrix calls"`.
- [ ] `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, tests of changed crates, `cargo deny check`.
- [ ] Ask the operator for the test server credentials and run `cargo test -p sdrmm-voicechat --test matrix_live -- --ignored`, plus a manual call from Element X to hear audio both ways.
