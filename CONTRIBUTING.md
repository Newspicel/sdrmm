# Contributing

Before we can merge your first pull request, sign the [Contributor License Agreement](CLA.md).
The CLA check comments on your first pull request; reply with the sentence it gives.
You keep your copyright; the CLA lets SDRmm also ship under a commercial license.

Read [AGENTS.md](AGENTS.md) for the rules, then run `cargo xtask check` and the tests for
what you changed. Add a changeset for user-visible changes:

```sh
cargo xtask changeset patch "RTL-SDR: keep gain after reconnect"
```
