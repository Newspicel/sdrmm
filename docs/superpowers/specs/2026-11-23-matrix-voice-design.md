# Matrix voice

## Why
The Voice chat node already joins Discord and TeamSpeak. Matrix is added as a third service, so a MatrixRTC call can hear the radio and talk out over the transmitter, the same way a Discord or TeamSpeak channel does.

## Goals
- A chat account of service **Matrix**. Voice chat nodes pick it like any other account.
- The node joins the MatrixRTC call of one room, through the room's LiveKit server, and trades 48 kHz mono audio with it both ways, the same as for Discord and TeamSpeak.
- Works with Element X and Element Web calls in encrypted and unencrypted rooms.

## Non-goals
- Video, screenshare, cameras (wanted later for all services).
- Legacy `m.call.*` 1:1 calls, the Element Call widget, text chat.
- More than one call per room per account.

## Anti-goals
- A call that looks joined but hears only noise (media keys missing or wrong). An undecryptable frame counts as a drop and shows.
- A new Matrix device on every server start.
- Any C or C++ media stack (libwebrtc, the official `livekit` crate).

## Constraints
- AGENTS.md: pure Rust preferred, newest stable deps, no `unwrap`, files under 3000 lines, functions under 200, a changeset, tests.
- `voicechat` has no internal deps. It already holds the Discord and TeamSpeak clients and Opus.
- The workspace uses `rusqlite` 0.41 (bundled). The released `matrix-sdk-sqlite` 0.19.1 needs `rusqlite` 0.40, and both link `sqlite3`, so they cannot build together. `matrix-sdk` main is already on 0.41.1.
- Project licence AGPL-3.0-or-later. `element-hq/matrix-rust-rtc` (AGPL-3.0-only, libwebrtc based, not on crates.io) may be read as a protocol reference, not depended on.

## Approaches
- **A. Pure Rust (picked).** `matrix-sdk` for login, sync, membership state and Olm to-device keys; our own LiveKit signalling client on `livekit-protocol` 0.9 types over `tokio-tungstenite`; `str0m` 0.26 with its `rust-crypto` backend (pure Rust DTLS through `dimpl`) for WebRTC; Opus through the existing `opus.rs`; frame E2EE written by hand with RustCrypto `aes-gcm` and `hkdf`.
- **B. Official `livekit` crate.** Least code, but wraps Google's prebuilt C++ libwebrtc. Rejected: pure Rust path exists.
- **C. webrtc-rs instead of str0m.** Pure Rust but mid-rewrite into the `rtc` crate. str0m is sans-I/O, steadier, and keeps control in our tasks.

## Decided
**Account (wire `ChatService::Matrix`):**
- A dedicated bot account, not a person's.
- `homeserver` (URL or server name, discovered via `.well-known`), `user` (`@name:server`), `login`, `nickname` (display name, empty keeps the account's own), `recovery_key` (secret, optional).
- `login` is one of:
  - **Password** (secret): `m.login.password`, which also works through Matrix Authentication Service's compat layer where enabled.
  - **OAuth** (MSC3861, next-gen auth through MAS): the device authorization grant (RFC 8628). The Accounts panel shows the verification link and user code; the operator approves in a browser. Access tokens expire and refresh.
- The homeserver's advertised auth decides which logins the form offers; the test server runs MAS, and both paths must work.
- The session (access token, refresh token, device id) is kept as a secret, so later starts restore the same device. `voicechat` hands every refreshed session back to the server, which writes it to the secrets store; `Session` gains that outward path (a callback or channel, shape left to the builder). A refused login or a revoked token is a red fault.
- On first login the bot bootstraps cross-signing (UIA with the password, or the OAuth flow's approval) so Element does not withhold keys from an unverified device. If the account already has cross-signing, the recovery key unlocks it; without one that is a red fault.
- `matrix-sdk` SQLite store (state and crypto) in the server data dir, one folder per account, opened with a passphrase kept as a secret. Deleting the account deletes its folder.
- Accounts panel gets a Matrix form: homeserver, user, login (password field, or the OAuth link and code), nickname, recovery key.

**Node:** `channel` takes a room id (`!id:server`) or alias (`#name:server`). The bot must already be in the room or be able to join it. Allowed takes Matrix user ids. Nothing else on the node changes.

**Joining (`voicechat::matrix`):**
1. Find the room's LiveKit focus: `GET /_matrix/client/v1/rtc/transports` (MSC4143), falling back to `.well-known` `org.matrix.msc4143.rtc_foci`.
2. Trade an OpenID token for a LiveKit JWT and URL at the focus's `lk-jwt-service` (MSC4195).
3. Send our call membership state event in the format current Element X sends; read both `m.rtc.member` and legacy `org.matrix.msc3401.call.member` from others. Use delayed events (MSC4140) for leave-on-crash where the server supports them, else membership expiry.
4. Open the LiveKit signal WebSocket, publish one Opus mic track, subscribe to every remote audio track.
5. On leave or drop of the link: unpublish, close, remove our membership. Membership is refreshed or expires per the MSC's delayed-event / expiry rules so a crash does not leave a ghost.

**LiveKit signalling (`voicechat::matrix::livekit`):** our own client for the subset we need: join, publisher and subscriber peer connections (or single PC if the server offers it), offer/answer, trickle ICE, add track, subscription updates, ping/pong, leave, reconnect. One `str0m::Rtc` per peer connection, driven by a tokio task owning a UDP socket (TCP fallback if LiveKit only offers it).

**Talkers:** a LiveKit participant identity is `@user:server:DEVICE`. A talker is verified only when that identity matches a current call membership of the room and, in an encrypted room, its media key arrived Olm-encrypted from that same device. Cross-signing trust of the talker is not checked. Its id is the Matrix user id, its name the room display name.

**E2EE (`voicechat::matrix::e2ee`):** used when the room is encrypted.
- Our 16-byte media key, random, sent Olm-encrypted to every participant device as `io.element.call.encryption_keys` (and the stable `m.rtc.encryption_key` when peers send it). As in matrix-js-sdk: a new member gets our current key; a member leaving rotates to a fresh key at the next index, sent at once and used 5 s later.
- Keys received the same way are kept per participant in a 256-slot key ring by index.
- Frames use LiveKit's frame-cryptor format: Opus frames keep their first byte clear, AES-GCM with the key derived by LiveKit's HKDF, IV and key index trailers. Derivation constants come from LiveKit's client SDK source.
- A frame with no key yet, or that fails to decrypt, counts in `Link::drops`.

**Fits the existing crate:** `Service::Matrix`, `Backend::Matrix`, `Session::join` returns the same `Link`. Rename sets the display name without leaving the call. Same backoff, `LinkState` and `Hold` behaviour as the other services.

**Docs and release:** Matrix in the voice chat docs; a minor changeset.

**rusqlite clash:** `matrix-sdk` and `matrix-sdk-sqlite` are pinned to a git rev of matrix-rust-sdk main (which brings ruma from git too). Back to crates.io once a release carries `rusqlite` 0.41.

**Target servers:** any spec homeserver with MSC4143 foci and `lk-jwt-service`; the test server runs Synapse behind MAS.

## Left to the builder
- Module split inside `voicechat::matrix`, keeping files under 3000 lines.
- Whether one or two peer connections, per what LiveKit server versions in use offer.
- Jitter buffer, packet loss handling and Opus FEC on decode.
- Tests: E2EE frame format against vectors produced with LiveKit's JS frame cryptor; signalling against a recorded or scripted fake SFU; membership and focus discovery against a mocked homeserver; an ignored live test against a real Synapse plus LiveKit when one is at hand.
