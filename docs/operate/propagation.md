---
title: Propagation map
description: Plot live FT8, FT4, and WSPR openings and estimate real-time MUF lower bounds with ionosonde overlays.
---

Wire `events` from FT8, FT4, or WSPR channels + station **GPS position** to **Propagation map** (preloads last 6 hours of logged decodes; grid-less reports/73 are ignored).

- **Activity:** heard stations per grid square weighted by count and **Half-life** (5 min–12 h, default 30 min).
- **MUF:** lower bound scaled to a 3000 km hop (`MUF(3000) >= f * M(3000) / M(D / hops)`). Paths < 500 km are excluded; set **Height** to `300 km` (F2) or `110 km` (sporadic E).
- **Paths:** great-circle arcs per grid/band (off by default).
- **Ionosondes:** overlays GIRO/INGV soundings from [prop.kc2g.com](https://prop.kc2g.com/) (15 min cache) and compares within 3000 km.
