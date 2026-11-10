---
bump: patch
---

A network bind without a token generates one, keeps it beside the database and logs a sign-in link, so Docker Compose starts with no setup. `--no-auth` runs it open on purpose
