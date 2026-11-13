# Configuration and security

`sdrmm` runs the interface, the receiver, and the REST, WebSocket, and MCP APIs in one process.
Out of the box it listens on `127.0.0.1:8080` with **no authentication**, so only this machine
reaches it. A bind other machines can reach always needs a [token](#token) and serves
[HTTPS](#https).

## Options

| Option | Default | Sets |
|---|---|---|
| `--bind <ADDRESS>` | `127.0.0.1:8080` | Listen address; a non-loopback one gets a token |
| `--db <PATH>` | `sdrmm/sdrmm.db` in the platform data folder | SQLite database |
| `--recordings-dir <PATH>` | `sdrmm/recordings` in the platform data folder | Recording folder |
| `--plugins-dir <PATH>` | `sdrmm/plugins` in the platform data folder | [Decoder plugins](../development/plugins.md) |
| `--token <TOKEN>` | Generated on a network bind | Shared access token |
| `--no-auth` | Off | Run a network bind without a token |
| `--tls-cert <PATH>`, `--tls-key <PATH>` | None | HTTPS certificate chain and key, PEM; always together |
| `--tls-self-signed` | On a network bind | HTTPS with a self-signed certificate |
| `--tls-name <NAME>` | Found addresses | Name the self-signed certificate must cover; repeatable or comma-separated |
| `--no-tls` | Off | Plain HTTP on a network bind, for a proxy or tunnel that adds HTTPS |
| `--dev-cors` | Off | Allow a separate frontend origin, for development only |
| `--doctor` | | Print diagnostics and exit |
| `--doctor-rates` | | Probe connected radios' sample rates and exit |

`SDRMM_TOKEN`, `SDRMM_NO_AUTH`, `SDRMM_TLS_NAMES` and `SDRMM_NO_TLS` set `--token`, `--no-auth`,
`--tls-name` and `--no-tls`. The platform data folder is `~/.local/share` on Linux,
`~/Library/Application Support` on macOS and `%APPDATA%` on Windows. For a service, use absolute
paths.

Subcommand: [`sdrmm phone`](#phones) pairs a phone.

## Data

The database holds workspaces, presets, bookmarks, saved radios, the recording index, the decoder
log, paired phones and [Event output](../user-guide/network-iq.md#credentials) credentials. The `tls` folder beside it holds the self-signed certificate, and
`access-token` the generated token. Recording files hold the signals. Back up all of them. Stop
the server before copying the database, or use SQLite's backup, and finish recordings first.

## Logs

The default level is `info,sdrmm=debug`. Change it with `RUST_LOG`:

```sh
RUST_LOG=info sdrmm
RUST_LOG=sdrmm=trace,info sdrmm
```

Trace logging is very verbose. Use it briefly.

## Token

Set a long random token before untrusted devices can reach the server:

```sh
export SDRMM_TOKEN='replace-with-a-long-random-secret'
sdrmm
```

The environment variable keeps the token out of the process list; `--token` works too. A network
bind without one makes a token on first start, keeps it in `access-token` beside the database and
logs a sign-in link. `--no-auth` runs it open instead, for a port only trusted machines reach. An
open server answers only to IP addresses, `localhost`, `--tls-name` names and, off loopback, its
own `<host>.local`. The browser asks for the token once and remembers it. API clients send
`Authorization: Bearer <token>`. WebSocket and download URLs can use `?token=...`.

Everything needs the token except the page itself, `/api/auth`, `/api/status`, `/api/about`,
`/api/openapi.json` and `/api/docs`. Every client with the token can do everything. There are no
user accounts or read-only roles. Browsers tell themselves apart with an `x-sdrmm-author` key,
which only scopes undo and names who switched workspaces.

## HTTPS

Loopback stays on plain HTTP. Any other bind serves HTTPS with a self-signed certificate, so the
browser warns once and you accept it. A plain `http://` request to that port is sent on to
`https://`. `--no-tls` keeps plain HTTP for a [tunnel](tunnels.md) or
[reverse proxy](#reverse-proxy) that adds HTTPS itself.

With your own certificate:

```sh
sdrmm --tls-cert /etc/sdrmm/fullchain.pem --tls-key /etc/sdrmm/privkey.pem
```

Both files are PEM, leaf certificate first. A missing or mismatched file stops startup.

The self-signed certificate covers `localhost`, `127.0.0.1`, `::1`, the server's LAN IPv4 addresses and
`<host>.local`. It is stored in `tls` beside the database, valid for 397 days and renewed at
startup after a year. Its key stays the same across renewals and name changes. Compare the
SHA-256 fingerprint in the log the first time a client trusts it.

In containers, behind NAT, or with a DNS name, list the names clients use. They replace the found
addresses:

```sh
sdrmm --bind 0.0.0.0:8080 --tls-name radio.example --tls-name 192.168.1.20
```

`SDRMM_TLS_NAMES` takes the same names, comma-separated. Changing the names creates a new
certificate.

## WebTransport

A server that serves HTTPS itself also takes WebTransport on the UDP port with the same number.
Chromium browsers use it and fall back to the WebSocket; other browsers, tunnels and proxies stay
on the WebSocket. Let that UDP port through your firewall. Audio then survives packet loss, and a
stalled picture no longer holds back the rest.

## Phones

[Phones](../user-guide/phones.md#allow-phones) pin the server's key when they pair. A server with
a self-signed certificate on a LAN address takes phones on its main port. Any other server needs
**Allow phones**, which opens a phone port, `8443` by default, with the self-signed key from
`tls`. A server with its own certificate needs it too, because a renewed certificate would break
every pin. The phone port takes paired phones only, and phones never get the shared token.

Pair from a terminal while the server runs:

```sh
sdrmm phone
sdrmm phone --name van --db /var/lib/sdrmm/sdrmm.db
```

It prints a QR code, the code, the key, the hosts and when the offer ends. `--db` must match the
running server, `--name` names the phone, and `--plain` or `NO_COLOR` drops the colours.

While phones can connect, the server announces itself as `_sdrmm._tcp` over mDNS, so the apps
list it under **Nearby**. Let the phone port through your firewall.

## Reverse proxy

- Bind SDRmm to loopback, or pass `--no-tls` and firewall its port.
- Set a token. Without one, the server answers only to the names under [Token](#token).
- Serve it at the root of the origin.
- Pass the original `Host` header. Browser requests whose `Origin` differs are refused.
- Forward WebSocket upgrades on `/api/ws`, and `/mcp` for MCP clients.
