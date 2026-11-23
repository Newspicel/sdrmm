---
title: Voice chat
description: Bridge Discord, TeamSpeak or Matrix voice channels to your radio and back with the Voice chat node.
---

```patch
[channel] NFM : Repeater input
[io] Voice chat : Account and channel
[encoder] NFM encoder : Audio FX + limiter
[output] Transmitter : Arm, level & timeout
[source] Device : Use = Send or Both

NFM --audio--> Voice chat
Voice chat --audio--> NFM encoder
NFM encoder --baseband--> Transmitter
Transmitter --transmit--> Device
```

## Accounts

Add accounts in **Library > Accounts**. They are server-wide, and every workspace picks from the same list. A voice chat offers only Discord, TeamSpeak and Matrix accounts.

| Kind | Fields |
|---|---|
| **Discord** | Bot token from the Discord developer portal, optional nickname. The bot needs **View Channel**, **Connect** and **Speak**, and **Change Nickname** for a nickname. |
| **TeamSpeak** | Address, optional password, nickname. The server makes the identity. **Clear** drops a saved password. |
| **Matrix** | Homeserver, user, login, optional recovery key. See [Matrix](#matrix). |

A new name or nickname applies live. Other changes reconnect.

## Voice chat node

| Setting | Meaning |
|---|---|
| **Account** | Which chat account to use |
| **Channel** | Discord: voice channel id. TeamSpeak: channel name or path, empty for the default channel. Matrix: room id or alias |
| **Allowed** | Discord: user ids. TeamSpeak: unique ids. Matrix: `@user:server` ids. Empty means anyone. A talker not yet known by id never matches |
| **To air** | What chat sends to the radio |
| **To chat** | What the radio sends to chat |

**To air** and **To chat** each have **Send** (**Voice activity** or **Continuous**), **Level**, and **Hang**. Voice activity opens above the level and stays open for the hang time.

## Matrix

Use a separate bot account on a homeserver with MatrixRTC and lk-jwt-service.

- **Login: Token** serves event output only. **Password** and **OAuth** serve voice only.
- **OAuth:** press **Sign in** in Accounts and approve the link.
- **Recovery key:** enter it when the account already has cross-signing.
- **Channel:** a room id or alias. The bot accepts an invite to the room.
- Media reaches LiveKit over UDP, or TCP when UDP is blocked. There is no TURN.

## Gateway

Wire channel `audio` into Voice chat, and Voice chat `audio` into an Encoder, then a Transmitter. Chat speech goes out on air, and received audio goes to chat.

- One voice channel per node.
- The node stays in its channel while it is on the canvas, wired or not.
- Continuous **To air** keeps an armed Transmitter keyed.
