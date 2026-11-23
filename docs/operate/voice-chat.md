---
title: Voice chat
description: Bridge Discord or TeamSpeak voice channels to your radio and back with the Voice chat node.
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

Add accounts in **Library > Accounts**. They are server-wide, and every workspace picks from the same list.

| Kind | Fields |
|---|---|
| **Discord** | Bot token from the Discord developer portal. The bot needs **View Channel**, **Connect** and **Speak**. |
| **TeamSpeak** | Address, optional password, nickname. The server makes the identity. **Clear** drops a saved password. |

## Voice chat node

| Setting | Meaning |
|---|---|
| **Account** | Which chat account to use |
| **Channel** | Discord: voice channel id. TeamSpeak: channel name or path, empty for the default channel |
| **Allowed** | Discord: user ids. TeamSpeak: unique ids. Empty means anyone. A talker not yet known by id never matches |
| **To air** | What chat sends to the radio |
| **To chat** | What the radio sends to chat |

**To air** and **To chat** each have **Send** (**Voice activity** or **Continuous**), **Level**, and **Hang**. Voice activity opens above the level and stays open for the hang time.

## Gateway

Wire channel `audio` into Voice chat, and Voice chat `audio` into an Encoder, then a Transmitter. Chat speech goes out on air, and received audio goes to chat.

- One voice channel per node.
- The node stays in its channel while it is on the canvas, wired or not.
- Continuous **To air** keeps an armed Transmitter keyed.
