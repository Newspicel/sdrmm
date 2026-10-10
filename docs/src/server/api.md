# API

REST, WebSocket, and MCP drive the same live receiver as the interface. Changes reach every
connected client.

| Endpoint | Serves |
|---|---|
| `/api/docs` | Swagger UI |
| `/api/openapi.json` | OpenAPI schema |
| `/api/ws` | WebSocket |
| `/mcp` | MCP over streamable HTTP |

The [OpenAPI schema](https://github.com/Newspicel/sdrmm/blob/main/openapi.json) is also in
the repository, for generating clients without a running server.

With a [token](configuration.md#token) set, send it on every request:

```sh
curl -H "Authorization: Bearer $SDRMM_TOKEN" http://receiver.local:8080/api/state
```

## REST

| Area | Routes |
|---|---|
| State and discovery | `/api/state`, `/api/devices`, `/api/saved-radios`, `/api/channeltypes`, `/api/clients`, `/api/patch/catalog` |
| Live receiver | `/api/devicesets`: device settings, channels, scanner, hunt, playback, time machine, network export |
| Workspaces | `/api/workspaces`: activate, apply, undo, redo, export, import |
| Saved setups | `/api/templates`, `/api/presets`, `/api/bookmarks` |
| Data | `/api/decoderlog`, `/api/recordings`, `/api/audiorecordings`, `/api/calls`, `/api/images`, `/api/occupancy` |
| Processing nodes | `/api/arrays`, `/api/radar`, `/api/fusion`, `/api/survey`, `/api/missions` |
| Tools | `/api/tools`, `/api/cps` (radio programmer), `/api/denoise-models` |
| Reference | `/api/bandplan`, `/api/satellites`, `/api/ionosonde`, `/api/position/nmea-devices` |
| Access | `/api/auth`, `/api/phones`, `/api/remote` |
| Server | `/api/status`, `/api/about`, `/api/doctor`, `/api/diagnostics` |

Errors are JSON with `error`, an optional `detail`, and an optional stable `code` such as
`not_found` or `auth`.

## WebSocket

Clients subscribe to streams over `/api/ws`. The server sends decoded records, scanner and hunt
progress, levels, node updates, state-change notices, and binary spectrum, audio, video, IQ,
symbol and surface frames. When it says some state changed, fetch that state again through REST.
Stream IDs belong to one connection. A client that sends `Present` joins the list of people
here and can send `Point` to share its pointer, selection, and drags.

A workspace `PUT` sent against an older revision is merged with what others wrote since. A channel
`PATCH` with a `base` lands only the fields changed from it. Undo and redo act on the caller's own
steps, named by the `x-sdrmm-author` header. The messages are `ClientCommand` and `ServerEvent` in the
OpenAPI schema; the web client in `web/src` is the reference implementation.

## MCP

Point an MCP client at `http://<server>:8080/mcp`, with the bearer header if a token is set. Its
tools edit the open canvas the way a user does: add, change and remove nodes, draw and cut wires,
tune radios, set channels, scan, and undo. Every change shows up on the canvas. It also reads the
decoder log and spectrum, and runs the [tools](../user-guide/tools.md). Tool arguments use the
same types as the OpenAPI schema.
