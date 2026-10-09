# 0001 — Projects live in the daemon, and a chat's workspace is optional

Date: 2026-10-08
Status: accepted

## Context

Until v0.2.0, "project" was a lie the sidebar told: a group label derived
from the basename of each chat's workspace folder. Nothing stored a
project; the folder was never registered anywhere. The console kept its
own "recents" list in localStorage, and every new chat required a
workspace folder (validated absolute + existing at `POST /sessions`).

The v0.2.0 milestone made projects real: a `+` in the sidebar and an
Add project button on the home screen register a folder; the sidebar
shows Projects (grouped) and Chats (flat); new chats pick a project or
run project-less.

Two decisions shaped the domain:

1. **Where does the registered project live?**
2. **Must every chat have a workspace?**

## Decision

**1. Projects are daemon-side entities** (`state.json` beside rules and
plugins, `GET/POST/DELETE /projects`), keyed by id, holding a `folders`
list — exactly one folder this release, so the multi-folder future
arrives additively with no data migration. The project's *name* is
never stored: it is its folder's basename, so the disk stays the only
place the name lives. A folder can register once; removing a project
unregisters it and touches nothing — the folder, and every chat that ran
in it, survive (the chats fall to the flat Chats section).

**2. `Session.workspace` is `Option<PathBuf>`** end-to-end. A project-less
chat carries no workspace at all — omitted on the wire, absent in
`state.json` — and spawns with its home folder as cwd. A named workspace
keeps its old bar: absolute, existing directory, 422 otherwise.
Automation rules keep a *required* workspace (a scheduled run needs a
concrete place); rules store the folder path of their chosen project.

## Alternatives rejected

- **Projects in console localStorage** (beside the recents they replace).
  Fastest to build, but automations, the pill, and any other machine
  would never see them — the daemon is the one source of truth every
  surface reads, and a project that only one browser profile knows about
  is not a real entity.
- **A single-folder `folder: PathBuf` field** instead of the `folders`
  list. Simpler now, but multi-folder projects are the stated next
  release; a list now costs nothing and avoids a state.json migration
  later.
- **Home-folder-as-workspace for project-less chats** (send `~` as the
  workspace, keep the field required). No daemon change, but it lies in
  the data: `~` would render as a project group named after the user, and
  the session record would claim a workspace the user never picked. The
  honest model is absence.

## Consequences

- `/projects` is machine-scoped like the rest of the API: in Cloud mode,
  a project registered on another machine is reached through
  `DaemonApi.device(id)` — the folder belongs to the machine that owns
  it, and the console's project list follows the chat's target machine.
- The console's recents are retired for new chats (nobody had installed
  the previous release - no migration); `openremote-recent-workspaces`
  stays readable by old builds but is written no more. The Settings
  workspace-defaults row (which configured the recents picker) is
  replaced by a Projects row.
- `session.workspace` consumers must handle `None`: the sidebar's flat
  section is the designated home for such chats, and the spawn paths fall
  back to the home folder for cwd.
