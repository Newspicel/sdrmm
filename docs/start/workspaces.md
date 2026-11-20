---
title: Workspaces and presets
description: Manage workspaces, collaboration, undo, templates, examples, presets, bookmarks, and band plans.
---

| Concept | Holds | Use for |
|---|---|---|
| Workspace | Nodes, wires, rack layout, radio settings, band plan | Full receiver setup (one active per server) |
| Template | Channels & outputs added to your selected Device | Quick setup on your radio |
| Example | Complete built-in workspace with explanatory Note | Learning a patch (**No radio** uses signal generator) |
| Preset | Settings & channels of open radios | Restoring radio state without changing nodes |
| Bookmark | Frequency, label, optional mode | One-click retuning |

## Workspaces

Switch, duplicate, export (JSON), and import workspaces from the top bar. Switching changes the active receiver for all connected clients (prompting if others are connected).

- **Collaboration:** everyone shares the live canvas with visible pointers and selections. Edits to separate nodes/fields merge; concurrent edits to the same field keep the last write.
- **Undo (`Ctrl`/`⌘ Z`):** per-author (100 steps) and live on the engine; rapid moves of one control collapse into one step.
- **Copy/paste (`Ctrl`/`⌘ C`/`V`):** preserves internal wires across workspaces; pasted Devices need a radio re-selected.
- **Notes:** **Tools → Note** stores markdown on the canvas and exports with the workspace.
- **Secrets:** webhook/DB/Matrix credentials stay in the server database, never in workspace JSON.

## Templates, examples, and band plans

- **Templates** (**Library → Templates**) retune your selected Device and add nodes; incompatible templates are greyed out.
- **Examples** (**Library → Examples**) import as new workspaces even if you lack their radios (Devices wait for replacements).
- **Bands** sets the band-plan region (default ITU Region 1), searches allocations, and toggles the Scope **Ruler** (click any allocation to tune).
