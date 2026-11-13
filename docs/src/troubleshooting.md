# Troubleshooting

Start with **Check hardware** on an empty Device node, or run:

```sh
sdrmm --doctor
```

It checks drivers, libraries, radio discovery, USB permissions, and storage paths.

## The page does not open

- Check the port in the `SDRmm ready` line of the server log.
- On the server itself, try <http://127.0.0.1:8080>.
- The desktop app only accepts its own computer. For other machines, run the `sdrmm` server with
  `--bind 0.0.0.0:8080` and a token; it stays on `127.0.0.1:8080` otherwise.
- Check the firewall and any container port mapping.
- Behind a reverse proxy, serve SDRmm at the root. Path prefixes do not work.
- A server on a network bind serves `https://` and sends `http://` there. Accept the
  certificate warning once.
- `Host not allowed`: a server without a token answers only to the
  [names under Token](server/configuration.md#token). Open it by IP address, or set a token.

## The window is blank or the waterfall is missing

Some graphics drivers break hardware rendering: a blank window, frozen panels, or
`waterfall unavailable: no WebGL2 context`. Start the app with GPU rendering off:

```sh
sdrmm-app --disable-gpu                    # Linux
open -a SDRmm --args --disable-gpu         # macOS
```

The waterfall then runs in software and may be slower. If the window still fails, run the
[portable server](getting-started/install.md#portable-server) with `--bind 127.0.0.1:8080` and use
a browser.

## The token is rejected

The browser forgets a rejected token and asks again. Enter the one the server is using now. API
clients send `Authorization: Bearer <token>`. URLs can carry `?token=...` instead.

## A radio is missing

1. Check that the operating system sees it.
2. Run `sdrmm --doctor` and fix any library or permission error.
3. Close other SDR programs that may hold it.
4. For SoapySDR radios, run `SoapySDRUtil --find` and check the module matches the core's
   version, 0.8 or 0.9.
5. On Linux, install the udev rules. In a container, set
   [`group_add`](server/deployment.md#usb-devices).

**SDRplay:** install the [SDRplay API](https://www.sdrplay.com/downloads/) and start
`sdrplay_apiService`. An RSPduo in use elsewhere only lists its free modes.

**Aaronia SPECTRAN:** install the RTSA-Suite PRO and close it before opening the radio. On Linux
run its `setup_udev.sh`.

See [Radios](hardware.md) for each radio's requirements.

## A radio is plugged in but its node stays disconnected

The node waits for the exact radio it saved: by serial, or by address for a network radio. If it shows **Open radio**, press it.
A node that says `stopped` was stopped by hand: press **Start**. To use a different radio, press
**Forget radio** and pick the new one.

## Spectrum works but audio is silent

- Wire channel `audio` to a Speaker and press play.
- If the Speaker shows **Resume audio**, press it. Browsers block audio until you click.
- Drag the squelch handle left. Far left turns it off.
- Check the channel sits on the signal and inside the Device's window.
- Check tab mute, system volume, and the output device.

Audio that stutters on a plain `http://` LAN address improves on HTTPS or localhost. Without
them the browser falls back to a slower audio path.

## A decoder shows nothing

- Check frequency, mode, and any baud rate or variant setting.
- Check the Scope shows a signal inside the channel.
- Wire `events` to the right place: Decoder log for messages, Readout for current state, Map for
  positions.
- Adjust gain. Watch for clipping and drops.
- Check the mode's [maturity](user-guide/decoders.md#catalog).

## Drops and gaps

The drop counter on a Device counts samples lost anywhere between the radio and the decoders.
Drops damage audio, spectrum, recordings, and decoding.

- Lower the sample rate and close channels and displays you do not need.
- From source, build with `--release`.
- On small computers, check for CPU throttling and heat.
- On Linux, allow a higher thread priority: add `@audio - nice -10` to
  `/etc/security/limits.conf` and join the `audio` group.
- Use wired Ethernet for network radios.
- Give fast USB radios their own USB bus. A HackRF at 20 MS/s nearly fills
  [USB 2](https://hackrf.readthedocs.io/en/stable/synchronization_checklist.html), and a shared
  hub can lose samples before any counter sees it.

Developers can measure loss on real radios with the
[capture health tests](development/hardware-tests.md#capture-health).

## Recordings do not appear

- Check the server can write to `--recordings-dir`.
- In Docker, check `/data/recordings` is on the persisted volume.
- Stop the recording. Metadata is written on stop.
- Check each capture has both `.sigmf-meta` and `.sigmf-data`.
