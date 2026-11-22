import type { Backends } from "./devices";

/** The built-in backends as `/api/devices` lists them, for tests. */
export const BACKENDS: Backends = [
  { backend: "rtltcp", label: "rtl_tcp", reach: "network", address_hint: "192.168.1.5:1234" },
  { backend: "spyserver", label: "SpyServer", reach: "network", address_hint: "192.168.1.5:5555" },
  {
    backend: "sdrconnect",
    label: "SDRconnect",
    reach: "network",
    address_hint: "192.168.1.5:5454",
  },
  { backend: "kiwisdr", label: "KiwiSDR", reach: "network", address_hint: "kiwi.example.org:8073" },
  { backend: "rtlsdr", label: "RTL-SDR", reach: "local" },
  {
    backend: "ad936x",
    label: "AntSDR / Pluto",
    reach: "network",
    address_hint: "192.168.1.10:30431",
  },
  { backend: "espsdr", label: "ESP-SDR", reach: "network", address_hint: "192.168.1.20:80" },
  { backend: "virtual", label: "Virtual", reach: "synthetic" },
  { backend: "recording", label: "Recording", reach: "playback" },
];
