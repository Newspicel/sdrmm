# Voice Chat Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A two-way Voice chat node (Discord, TeamSpeak) on server-wide chat accounts, a shared voice gate with send modes, and mixed transmitter audio.

**Architecture:** `dsp` gains `MixBus` and `VoiceGate`. A new `sdrmm-voicechat` crate turns an account into sessions and voice links (plain channels of 20 ms mono 48 kHz frames). The server keeps chat accounts in `sdrmm.db`, follows the patch to join channels, and feeds transmitters through a lane-aware mixer in the engine.

**Tech Stack:** Rust (tokio, songbird 0.6 + twilight 0.17, tsclient-rs 0.3, earshot 1.2, opus 0.4), React/TypeScript web.

**Spec:** `docs/superpowers/specs/2026-11-22-voice-chat-design.md`

## Global Constraints
- AGENTS.md: no `//` comments, `///` only for rationale; no `unwrap`/`expect` outside tests and startup; functions under 200 lines, files under 3000; short UI text, no em dash.
- Controls: dropdowns for choices, fields for numbers, sliders for levels, name small above. No switches or checkboxes.
- Red only for real faults; a missing wire or account is an empty choice, never an error.
- Hot DSP path: no locks, no allocation per sample, no async.
- `sdrmm-voicechat` has no internal deps and is in `FORBIDDEN_ON_PHONES`.
- Discord: songbird `default-features = false, features = ["driver", "gateway", "twilight", "rustls", "receive"]`, twilight-gateway/model/http 0.17.
- Voice gate defaults: threshold -35 dB (range -90..0), hang 600 ms (max 5000).
- Do not commit: the operator commits. "Commit" steps mean: leave the work staged-ready and report the files.
- Finish each task with `cargo fmt`, `cargo clippy -p <crate> --all-targets`, tests of what changed.

## Review Focus
- Account deleted while a node uses it: node shows an empty account choice and leaves the channel.
- Account edited (token or address changed): the session reconnects with the new values.
- Microphone and Voice chat feeding one transmitter at once: one mixed stream, not interleaved (engine test).
- A TeamSpeak client claiming an allowed nickname: not allowed, only unique ids match (test in to_air).
- Workspace from another server with an unknown account id: loads, node shows empty choice (wire test).

---

### Task 1: dsp MixBus and VoiceGate
**Files:** Create `crates/dsp/src/audio/mix.rs`, `crates/dsp/src/audio/voice_gate.rs` (tests inline); modify `crates/dsp/src/audio.rs`, `crates/dsp/Cargo.toml` (earshot), workspace `Cargo.toml`.

**Produces:**
- `pub fn lane_of(name: &str) -> u64` (FNV-1a 64).
- `MixBus::new(max_lag: usize)`, `push(&mut self, lane: u64, samples: &[f32])`, `drain(&mut self, out: &mut Vec<f32>)` appends released samples clamped to [-1, 1], `forget(&mut self, lane: u64)`. Release point: the lowest write cursor among lanes within `max_lag` of the leading lane. A lane past `max_lag` behind stops being waited for and rejoins at the release point when it pushes again.
- `pub const VOICE_GATE_RATE_HZ: u32 = 48_000`; `VoiceGate::new(threshold_db: f32, hang_ms: u32)`, `set(&mut self, threshold_db: f32, hang_ms: u32)`, `feed(&mut self, block: &[f32]) -> bool`, `level_db(&self) -> f32`. Opens when the block peak is at or above the threshold and earshot's score (48 kHz decimated by 3 into 256-sample frames) is at least 0.5 in the last 3 frames; stays open `hang_ms` after the last such block.

- [ ] Tests: `one_lane_passes_through_at_once`, `two_lanes_sum_in_time`, `a_silent_lane_stops_holding_the_mix_after_max_lag`, `the_mix_is_clamped`, `gate_stays_shut_below_threshold`, `gate_holds_for_hang_then_shuts`, `steady_hiss_does_not_open_the_gate`, `lane_of_is_stable`.
- [ ] Implement, run `cargo test -p sdrmm-dsp audio::mix audio::voice_gate`, commit.

### Task 2: wire types
**Files:** Create `crates/wire/src/voice_chat.rs` (tests in `crates/wire/src/voice_chat/tests.rs`); modify `crates/wire/src/transmit.rs`, `crates/wire/src/lib.rs`, `crates/wire/src/patch/registry.rs`, `crates/wire/src/patch/resolve/checks.rs`, `crates/wire/src/ws.rs`.

**Produces:**
- `enum SendMode { PushToTalk, VoiceActivity, Continuous }` (snake_case).
- `struct VoiceGateSettings { mode: SendMode, threshold_db: f64, hang_ms: u32 }`, default VoiceActivity / -35 / 600, `valid()`.
- `MicrophoneNode { send: SendMode, threshold_db, hang_ms }`, default PushToTalk; loads old `vox: true` as VoiceActivity, `vox: false` as PushToTalk; `gate() -> VoiceGateSettings`.
- `VoiceChatNode { account: String, channel: String, allowed: Vec<String>, to_air: VoiceGateSettings, to_chat: VoiceGateSettings }`, `valid()` refuses PushToTalk and over-long strings (account 64, channel 255, 64 allowed of 64 chars).
- `ChatAccount { id, name, #[serde(flatten)] service: ChatService }`; `ChatService::Discord { token: Secret }` | `ChatService::Teamspeak { address: String, password: Secret, nickname: String, identity: Secret }` (tag `service`). `ChatAccountsResponse { accounts: Vec<ChatAccount> }`.
- `VoiceChatStatus { node, state: VoiceChatState, error: Option<String>, talking: Vec<ChatTalker>, to_chat_db: Option<f32>, to_air_db: Option<f32>, to_chat_open: bool, to_air_open: bool }`, `VoiceChatState { Idle, Connecting, Joined }`, `ChatTalker { id, name, allowed: bool }`; `ServerEvent::VoiceChatStatus`; `ProblemSource::VoiceChat`.
- Registry: `(VoiceChat(VoiceChatNode), "voice_chat", Output, "Voice chat", "Discord or TeamSpeak, both ways", Role::Plain, Face::fit(340), [audio_in(), audio_out()])`.

- [ ] Tests: `old_vox_microphone_loads_as_voice_activity`, `old_microphone_without_vox_loads_as_push_to_talk`, `voice_chat_refuses_push_to_talk`, `voice_chat_with_unknown_account_is_valid`, `chat_account_round_trips`, registry test that `voice_chat` has `audio` in and out.
- [ ] Implement, `cargo test -p sdrmm-wire`, commit.

### Task 3: engine mixes transmitter audio
**Files:** Modify `crates/engine/src/transmit/voice.rs`, `crates/engine/src/transmit.rs`, callers in `crates/server/src/transmit.rs`.
**Consumes:** `MixBus`, `lane_of`. **Produces:** `Engine::transmit_audio(&self, ds: u32, node: &str, lane: u64, pcm: Vec<f32>)`; `Voice::speak(lane, pcm)`; Voice thread mixes with `MixBus::new(4_800)`.
- [ ] Test `two_sources_on_one_voice_are_mixed_not_interleaved` (two lanes of 4800 samples each → 4800 out, summed).
- [ ] Implement, `cargo test -p sdrmm-engine transmit`, commit.

### Task 4: server microphone gate
**Files:** Modify `crates/server/src/transmit.rs` (gates per microphone node in `TransmitHub`), `crates/server/src/tests/transmit.rs`.
**Produces:** `transmit::speak(state, microphone, pcm)` applies the node's gate: PushToTalk and Continuous pass, VoiceActivity feeds `VoiceGate`. `transmit::speak_as(state: &AppState, source: &str, lane: u64, pcm: &[f32]) -> usize` carries audio from any node whose audio out reaches armed transmitters through an Encoder (used by Task 7); `speak` calls it after its gate.
- [ ] Tests: `voice_activity_microphone_drops_quiet_frames`, `continuous_microphone_passes_quiet_frames`.
- [ ] Implement, test, commit.

### Task 5: chat accounts store and REST
**Files:** Create `crates/server/src/store/chat_accounts.rs`, `crates/server/src/rest/chat_accounts.rs`; modify `crates/server/src/store.rs` (table `chat_accounts(id TEXT PRIMARY KEY, body TEXT NOT NULL, updated_at)`), `crates/server/src/rest.rs`, `openapi.json`.
**Produces:** `GET/POST /api/chat-accounts`, `PUT/DELETE /api/chat-accounts/{id}`. Inline secrets move into the secrets store on save; a new TeamSpeak account gets `identity` from `sdrmm_voicechat::new_teamspeak_identity()` stored as a secret. Store exposes `subscribe_chat_accounts() -> watch::Receiver<u64>`.
- [ ] Tests: `created_account_keeps_token_out_of_the_body`, `teamspeak_account_gets_an_identity`, `deleting_an_unknown_account_is_404`.
- [ ] Implement, test, commit.

### Task 6: sdrmm-voicechat crate
**Files:** Create `crates/voicechat/{Cargo.toml,src/lib.rs,src/link.rs,src/discord.rs,src/teamspeak.rs,src/opus.rs}`; modify workspace `Cargo.toml`, `xtask/src/architecture.rs` (`("sdrmm-voicechat", &[][..])`, phone forbidden), AGENTS.md crate list.
**Produces:**
- `pub const RATE_HZ: u32 = 48_000; pub const FRAME: usize = 960;`
- `enum Service { Discord { token }, TeamSpeak { address, password: Option<String>, nickname, identity } }`
- `struct Talker { id: String, name: String }`, `struct Heard { talker: Talker, pcm: Vec<f32> }` (mono, 48 kHz).
- `enum LinkState { Connecting, Joined, Failed(String) }`
- `struct Link { pub send: mpsc::Sender<Vec<f32>>, pub heard: mpsc::Receiver<Heard>, pub state: watch::Receiver<LinkState> }`; dropping it leaves the channel.
- `Session::open(service: Service) -> Session`; `Session::join(&self, channel: &str) -> Result<Link, VoiceChatError>`; `VoiceChatError::Clash` when a TeamSpeak session already sits in another channel. Reconnects with backoff 1 s doubling to 60 s.
- `pub fn new_teamspeak_identity() -> String`.
- Discord: one twilight shard per session; channel id → guild via twilight-http; songbird join; heard from `VoiceTick` decoded audio, talker id = user id from SSRC map, name via member cache or id. TeamSpeak: `ClientOptions.default_channel`, `send_voice(opus, 4)`, decode per client with `opus`, talker id = client unique id from the directory.
- [ ] Tests: `opus_round_trip_keeps_a_tone`, `teamspeak_identity_is_reusable`, `second_teamspeak_channel_clashes` (no network: clash check before connect).
- [ ] Implement, `cargo test -p sdrmm-voicechat`, `cargo xtask` architecture check, commit.

### Task 7: server voice_chat
**Files:** Create `crates/server/src/voice_chat.rs`, `voice_chat/{hub.rs,to_chat.rs,to_air.rs,status.rs,tests.rs}`; modify `crates/server/src/lib.rs`.
**Consumes:** Tasks 1-6. **Produces:** `voice_chat::follow(state)` like `speakers::follow`. Hub: one `Session` per account in use, rebuilt when the account changes, dropped when unused. Per node: join when a port is wired; to_chat runs while the in port has wires (subscribe routes as Speaker does, mono mix, gate, 960-sample frames); to_air runs while the out port reaches an armed transmitter (allowed filter on talker id, `MixBus` per talker, gate, `speak_as`). Clash and failures go to `ProblemSource::VoiceChat`; status to `ServerEvent::VoiceChatStatus` once per second and on change. Network sits behind `trait Links { fn join(&self, account: &ChatAccount, channel: &str) -> Result<Link, String> }`.
- [ ] Tests (fake Links): `unwired_node_stays_out_of_the_channel`, `only_allowed_talkers_reach_the_transmitter`, `empty_allowed_lets_anyone_through`, `same_account_same_channel_twice_is_a_fault`, `continuous_to_chat_sends_silence`.
- [ ] Implement, test, commit.

### Task 8: web
**Files:** Create `web/src/canvas/nodes/VoiceChatFace.tsx`, `web/src/canvas/nodes/voiceChat.ts` (+tests), `web/src/components/AccountsPanel.tsx` (+test), `web/src/canvas/nodes/GateControls.tsx`; modify `MicrophoneFace.tsx`, `useMicrophone.ts`, `lib/microphone.ts`, `lib/api.ts`, `lib/types.ts`, `canvas/nodes/index.tsx`, panel mount, `generated/schema.d.ts` (regenerate).
**Produces:** Accounts panel (list, add Discord or TeamSpeak, edit, delete). Voice chat face: Account dropdown, Channel field, Allowed list, To air and To chat: Send dropdown, Level slider, Hang field; readouts: state, meters, talking list with copyable ids. Microphone: Send dropdown (Push to talk, Voice activity, Continuous); browser gate removed; capture only while `reachesArmedTransmitter(node)` and (keyed or mode is not push to talk).
- [ ] Tests: `voiceChat.test.ts` gate edits keep other fields; `reachesArmedTransmitter` true/false cases; AccountsPanel renders saved accounts without secrets.
- [ ] `npm run check` / `npm test` for touched files, commit.

### Task 9: docs and release
**Files:** Create `docs/operate/voice-chat.md`; modify `docs/operate/transmit.md` (Send modes), `docs/index.md` nav if listed; `cargo xtask changeset minor "Voice chat node for Discord and TeamSpeak"`.
- [ ] Commit.
