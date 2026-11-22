---
title: Sign-in and roles
description: Sign in with an OIDC provider such as Pocket ID, Authelia, Authentik or Keycloak, and map its groups to roles.
---

Each person signs in with their own account. Their groups decide what they may do:

| Role | May |
|---|---|
| **Viewer** | Watch and listen. The canvas is read-only. |
| **Operator** | Change workspaces, tune, decode, transmit. |
| **Admin** | Everything, plus phones, chat accounts, secrets and diagnostics. |

The [shared token](configuration.md#token) keeps working beside sign-in and holds the admin role, so scripts, MCP clients and a provider outage never lock you out.

## Set up the provider

Create an OIDC client with:

- **Redirect URL:** `https://sdr.example.org/api/auth/callback`
- **Logout URL** (optional): `https://sdr.example.org/`
- **Scopes:** `openid profile email groups`

A public client works too (PKCE is always used); leave the secret unset.

## Start SDRmm

```sh
export SDRMM_OIDC_CLIENT_SECRET='from-the-provider'
sdrmm --bind 0.0.0.0:8080 \
  --public-url https://sdr.example.org \
  --oidc-issuer https://id.example.org \
  --oidc-client-id sdrmm \
  --oidc-admin-group sdr-admins \
  --oidc-operator-group radio
```

| Flag | Env var | Purpose |
|---|---|---|
| `--public-url` | `SDRMM_PUBLIC_URL` | Address browsers open SDRmm at |
| `--oidc-issuer` | `SDRMM_OIDC_ISSUER` | Provider issuer URL |
| `--oidc-client-id` | `SDRMM_OIDC_CLIENT_ID` | Client ID |
| `--oidc-client-secret` | `SDRMM_OIDC_CLIENT_SECRET` | Client secret, if any |
| `--oidc-admin-group` | `SDRMM_OIDC_ADMIN_GROUPS` | Groups granted admin (repeatable / comma-separated) |
| `--oidc-operator-group` | `SDRMM_OIDC_OPERATOR_GROUPS` | Groups granted operator |
| `--oidc-viewer-group` | `SDRMM_OIDC_VIEWER_GROUPS` | Groups granted viewer |
| `--oidc-default-role` | `SDRMM_OIDC_DEFAULT_ROLE` | Role for users in none of the groups (default: refused) |
| `--oidc-groups-claim` | `SDRMM_OIDC_GROUPS_CLAIM` | Claim holding the groups (default `groups`) |

The highest matching group wins. Groups are read from the ID token, or from the userinfo endpoint when the token has none.

Sessions last 12 hours and survive restarts. A role change at the provider applies at the next sign-in. Sign out from the people menu at the top right.

> [!NOTE]
> With Tailscale, run [tsidp](https://github.com/tailscale/tsidp) as the provider to sign in with tailnet identities.
