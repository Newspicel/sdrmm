# Voice chat node

## Why
People in a Discord or TeamSpeak voice channel hear the radio and talk out over the operator's transmitter. The node is a two-way gateway between a voice chat and the patch.

## Goals
- One **Voice chat** node. Service dropdown: Discord, TeamSpeak.
- `audio` in (many wires): channel audio spoken into the voice channel.
- `audio` out: everyone in the voice channel, mixed. Above the voice gate it reaches every armed transmitter its Encoder feeds, the way a keyed Microphone does.
- Allowed field: who may key. Empty means anyone.
- Chat accounts are server-wide, outside workspaces: one list of Discord bots and TeamSpeak clients that any workspace's nodes pick from. Event output will later send messages through the same accounts.

## Non-goals
- Text chat, bot commands, joining several channels from one node.
- Wiring the out port into Speaker, Audio recorder or decoders (needs engine PCM routes for non-channel sources). The port accepts what Microphone's out accepts: Encoder.
- Phones.

## Anti-goals
- A stranger keys the transmitter when Allowed is set.
- A dead connection that looks alive. A lost or refused connection is a red fault on the face.

## Constraints
- AGENTS.md rules: one node, settings via the registry form, dropdown/field/slider controls only, no long text on the face, secrets in the server store, no `unwrap` outside tests, files under 3000 lines.
- Discord allows only bot accounts and requires DAVE E2EE since March 2026.
- New crate boundaries must be added to `xtask/src/architecture.rs`; the crate must stay off phone builds (`FORBIDDEN_ON_PHONES`).

## Approaches
- Accounts are server-wide, picked by id, so one bot serves many workspaces and later Event output; a node-held token would mean one connection per node and per-workspace credentials.
- **A. Grow Speaker into one Audio output node** with Discord/TeamSpeak as Out choices. Rejected by the operator: a voice chat is two-way and has its own settings, so it is its own job.
- **B. Separate Voice chat node, two-way (picked).** Speaker stays local playback. Voice chat owns the connection, both directions and the gate.
- Libraries: Discord via `songbird` 0.6 (DAVE through `davey`) plus `twilight-gateway` for the bot login; writing Discord voice and DAVE by hand is out of proportion. TeamSpeak via `tsclient-rs` 0.3 (MIT, pure Rust, maintained; `moqspeak/ts3proto` is a fork of it). Opus encode/decode where a library does not do it: the workspace `opus` crate.

## Decided
**Chat accounts (server-wide, wire type `ChatAccount`):**
- Kept in `sdrmm.db` beside secrets, with REST CRUD under `/api/chat-accounts` and an **Accounts** panel in the web app like Presets or Bookmarks. Never exported with a workspace.
- Each has an id, a name and a service:
  - Discord bot: `token` (secret).
  - TeamSpeak client: `address` (host[:port], SRV/TSDNS resolved by the library), `password` (secret, optional), `nickname` (default `sdrmm`), identity generated on create and kept in the secrets store.
- The server holds one live session per account in use, shared by every node that uses it (Voice chat now, Event output later). An account no node uses stays offline.
- A Discord bot can be in one voice channel per guild, a TeamSpeak client in one channel. A second node asking the same account for another channel gets a red fault naming the clash.

**Node settings (wire type, `VoiceChatNode`):**
- `account`: chat account id (dropdown). The node's service follows the account. An id this server does not know shows as an empty choice, the same as a new node.
- `channel`: Discord voice channel id (the server is looked up from it), or TeamSpeak channel name or path.
- `to_air` and `to_chat`: each a Send mode (Voice activity | Continuous) with its own `threshold_db` (slider, default -35, range -90..0) and `hang_ms` (field, default 600, max 5000).
- `allowed`: list of strings. Discord user IDs; TeamSpeak unique identities (nicknames are not trusted, anyone can take one). Empty: anyone. The "talking now" readout shows each speaker's id so it can be copied.

**Server:** a chat-account hub owns sessions (connect on first use, drop on last, reconnect with backoff, follow account edits). A `voice_chat` module follows the resolved patch like `speakers.rs` and asks the hub for a voice channel per node.

**In direction:** subscribe each wired channel's PCM (same `AudioRoute` and FX handling as Speaker), mix to mono 48 kHz, run the `to_chat` gate, encode Opus 20 ms frames, send. Below the gate nothing is sent.

**Out direction:** decode each speaker, drop speakers not in Allowed, mix to mono 48 kHz. The `to_air` gate on the mix decides when frames go to `transmit::speak`-style delivery: every armed transmitter reached from this node's out port through an Encoder. Below the gate nothing is sent, so the transmitter unkeys after its own gap. Transmitter timeout still caps one transmission.

**Voice gate (shared, in `dsp`):** `VoiceGate` ported from moqspeak's capture gate: level against a threshold, the pure Rust `earshot` speech detector, then a hang. Modes: Push to talk, Voice activity, Continuous. Voice chat offers Voice activity and Continuous on both directions (Continuous to air keeps an armed transmitter keyed). Microphone gets a Send dropdown with all three and its voice activity moves from the browser to the server, so one gate serves every node. A workspace with `vox: true` loads as Voice activity, `vox: false` as Push to talk.

**Only live when wired:**
- Microphone captures only while its out port reaches an armed transmitter through an Encoder. Otherwise the browser mic stays closed, in every mode.
- Voice chat joins its channel only while one of its ports is wired; it decodes chat audio while joined, for the talking readout, delivers it only while its out reaches an armed transmitter, and encodes only while its in carries a wire.
- An account's session runs only while some node needs it.

**Several sources on one transmitter:** Microphone, Voice chat nodes and wired channels feeding one armed transmitter are mixed into one stream in front of its voice queue in the engine (sum with a limiter, per-source buffers aligned on time), never interleaved.

**One voice channel, one node:** a second node on the same account and channel gets a red fault. The input is never muted while the node keys: a chat client hears none of its own audio, and wiring a receiver into the chat is the operator's patch.

**Face:** state (connecting, in channel, red fault), meter for audio in, meter for chat out with the gate mark, who is talking now. Empty states keep the face's shape.

**Docs and release:** a section in `docs/operate/transmit.md` or a new `docs/operate/voice-chat.md`; a minor changeset.

## Left to the builder
- Exact module and crate split (a new `sdrmm-voicechat` crate for the protocol clients, with server glue in `crates/server/src/voice_chat`, is the expected shape).
- Reconnect backoff values, jitter buffer size for decode, the mix's limiter.
- How problems map to `ProblemSource` (add `VoiceChat`).
- Test strategy: unit tests for gate, mix, allowed filter, settings validation; protocol clients behind a trait so the server logic tests without a network.
