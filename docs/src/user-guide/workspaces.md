# Workspaces and presets

| | Holds | Use it to |
|---|---|---|
| Workspace | Nodes, wires, rack, radio settings, band-plan region | Keep a whole receiver |
| Template | A ready-made receiver, built in | Start a common setup |
| Example | A whole working workspace, built in | Learn which nodes a job needs |
| Preset | Settings and channels of every open radio | Get back to a known state |
| Bookmark | A frequency, a name, and an optional mode | Retune quickly |

## Workspaces

Create, switch, rename, duplicate, export, and delete workspaces from the name in the top bar. A
new database starts with a Device wired to a Scope, and a Speaker. Later workspaces start empty.
Changes save automatically.

One workspace runs per server. Switching changes it for every client, including
[phones](phones.md), which can switch it too. When others are connected you confirm first, and
they see who switched.

### Working together

Everyone on the same server edits the same workspace. The top bar shows who is here; click your own
circle to set your name. Other people's pointers, selections, and drags show live on the canvas.
Edits to different nodes, or different settings of one node, never overwrite each other. When two
people change the same setting, the later one wins.

### Undo

Use the top-bar arrows, `Ctrl`/`⌘ Z`, and `Ctrl`/`⌘ Shift Z`. Undo takes back your own last change
and keeps what others did since. It changes the running receiver: undoing an added channel closes
it, and undoing a dial move tunes back. Moves of one control within a second count as one step.
The server keeps 100 steps per workspace.

### Copy and paste

Select nodes, then `Ctrl`/`⌘ C` and `Ctrl`/`⌘ V`. Copies land beside the originals with the wires
between them. Pasted Device and Recording nodes need a radio or file picked. The clipboard works
across workspaces while the tab stays open.

### Notes

A **Note** node, under Tools, holds markdown: headings, bold, code, lists, quotes, rules, and
links. Double-click it to write, click away to keep. It travels with the workspace, so an exported
file carries its own instructions. Pin it to the rack to keep it in view.

### Export and import

The download button beside a workspace saves it as JSON, with its tuning. **Import a workspace
file** adds it as a new workspace and switches to it. Radios that are present open with the saved
settings. Missing ones stay disconnected and show *radio not connected*, so you can pick
replacements.

## Templates

Select a Device, or have only one, then open **Library → Templates** and press **Apply**. A
template retunes that radio, sets its rate, and adds channels and outputs. Templates the radio
cannot run are greyed out.

Undo removes the added nodes but leaves the radio's new frequency and rate.

## Examples

**Library → Examples** holds whole workspaces. **Import** adds one as a new workspace and switches
to it, leaving the open one alone. A template changes the radio you already have; an example
arrives with its own.

Each row says what the example needs, and names the nodes it uses, so you can go the other way and
find the example that shows a node you have not used. Nothing is greyed out: an example you lack
the radios for still imports, and its Device and Recording nodes stand empty and say so, ready for
you to pick replacements.

Every example carries a **Note** naming each of its nodes and why it is there. The note travels
with the workspace, so it survives your changes and an export.

**No radio** runs on a Signal generator, so there is something to take apart before any hardware
arrives.

## Presets

A preset saves every open radio as it is now: its settings and its channels. Applying it puts them
back on the matching radios open in the active workspace. It does not add or remove nodes.

## Bookmarks and band plans

**Bookmarks** saves the selected Device's or channel's frequency under a label and tunes it back.

**Bands** picks the workspace's band-plan region and searches its allocations. The default is ITU
Region 1. A hit tunes the selected Device, or the selected channel, moving its radio if needed.
**Ruler** in Bands, or **Band plan** in a Scope's settings, draws allocations on every Scope. Hover
one for details, click it to tune.
