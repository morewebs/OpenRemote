# The Worktrunk plugin

OpenRemote's official plugin for [Worktrunk](https://worktrunk.dev)
(`wt`, github.com/max-sixty/worktrunk), the git worktree manager built for
running agents in parallel. Its source is
[`examples/plugins/worktrunk`](../../examples/plugins/worktrunk), and it is
listed in OpenRemote's catalog (`.claude-plugin/marketplace.json`).

## What it is for

Several agents on one repository need a folder each, or they trip over each
other's files. Worktrunk gives every branch its own worktree, and merges
and cleans them up when the work is done. This plugin lets an agent do that
from a chat, lets a person do it from a view, and makes a chat's branch
visible wherever the chat is shown.

## What it adds

| Where | What |
|---|---|
| Agent tools | `worktrunk__list`, `status`, `create`, `switch`, `remove`, `merge`, `start_chat`, `version`, `help`, `run` |
| Sidebar, Plugins | **Worktrees** view: Worktrunk's version and updates, your repositories, their worktrees (new chat, merge, remove), new worktree with or without a chat, hook approvals |
| Composer | **Worktrees** button: opens the view |
| Every chat (console) | Its branch in the header, and "worktree of *repo*" for a linked worktree; a chat in a linked worktree shows its branch in the sidebar |
| Settings > Plugins > Worktrunk | Worktrunk program, hooks, harness for new chats, update checks |

### The tools

- **list**: the repository's worktrees, one line each (branch, folder,
  ahead/behind the default branch, uncommitted changes, last commit),
  then the full JSON Worktrunk printed. `branches`, `remotes`, `full` add
  what `wt list` adds.
- **status**: the chat's own worktree.
- **create** / **switch**: `wt switch [--create]`, with `--no-cd`. The
  answer is the worktree's folder: the chat's own folder doesn't move
  (an agent works there by full path, or hands the work on).
- **start_chat**: creates the worktree (or uses an existing one) and
  starts a chat there with a prompt, on the asking chat's harness and
  model unless told otherwise. The parallel-agents move.
- **remove**: `wt remove --foreground` by branch. Never the chat's own
  worktree unless `allow_this_chat`.
- **merge**: `wt merge` (commit, squash, rebase, pre-merge hooks,
  fast-forward, cleanup; never pushes). From the chat's own worktree it
  keeps the worktree unless `remove: true`, so a chat is never left in a
  folder that is gone.
- **version**: the installed Worktrunk, how it was installed, its
  commands, the newest release, and the update command.
- **help**: Worktrunk's own `--help` for any command, from the installed
  version.
- **run**: any other `wt` command (`step diff`, `step prune --dry-run`,
  `config show`, and commands Worktrunk adds later), arguments passed as
  they are.

## Keeping up with Worktrunk

Nothing about one Worktrunk version is built in.

- **Flags come from the installed version's own help.** Before passing a
  flag (`--no-cd`, `--format`, `--foreground`, `--no-hooks`), the plugin
  checks that this version's `--help` lists it, read once per version and
  read again when the program file changes. A newer Worktrunk works the
  day it ships; an older one is never handed a flag it doesn't know (the
  answer then falls back to its text, or to a `wt list` lookup).
- **Output is read leniently.** `wt list` JSON schema 2 (the envelope)
  and schema 1 (the bare array) both read; fields the plugin doesn't know
  pass through to the agent untouched; missing fields are fine.
- **New commands reach agents without a plugin update** through `help`
  and `run`, and `version` lists the installed version's commands.
- **Updates.** Every few hours (setting **Check for Worktrunk updates**)
  the plugin reads the newest release from Worktrunk's git tags (`git
  ls-remote`, no API or token). The view shows "Worktrunk x.y.z is out"
  with an **Update** button:
  - a copy installed by winget, Homebrew or `cargo install` is updated by
    that manager (`winget upgrade --id max-sixty.worktrunk`, `brew upgrade
    worktrunk`, `cargo install worktrunk --locked`), so the manager's
    records stay right;
  - a manager that needs a password or a prompt (pacman, conda, pixi, the
    install script) is named, not run;
  - the plugin's own copy is replaced by the new release (below).
- **Installing.** Without Worktrunk, the view offers **Install
  Worktrunk**: the release archive for this system is downloaded with
  `curl` over https from Worktrunk's GitHub releases, checked against the
  release's published SHA-256, refused if it holds links, unpacked with
  `tar` into the plugin's private data folder, and run only after its
  `--version` says the expected version. An update puts the new version
  beside the old and moves a pointer, so a failed update leaves the
  working copy alone. Systems without a published archive (Windows on
  ARM) are told to use `cargo install`.

## Finding Worktrunk

The plugin's program gets a cleared environment and never runs a bare
`wt`. In order:

1. the full path in **Worktrunk program**, if set;
2. the plugin's own copy;
3. PATH, then the usual install folders (`~/.cargo/bin`, winget's Links,
   Homebrew's, `~/.local/bin`), trying `git-wt` before `wt` on Windows.

Windows Terminal's `wt.exe` (anything under `WindowsApps`) is never
taken, and every candidate must answer `--version` as Worktrunk does. The
program found is then run by its absolute path.

## What it will not do, and why

- **No shell, ever.** Every argument is one argv entry. Branch names are
  checked as git checks them and may not start with `-`; flag values may
  not be flags.
- **No command running behind the user.** Worktrunk can run programs:
  `-x/--execute`, project hooks, `wt hook`, `step for-each`, `step
  tether`, aliases. None of them is reachable by an agent's arguments:
  `run` refuses `-x`, `--execute`, `-y`, `--yes`, `--config`,
  `--config-set`, `--` and those commands, and `config` beyond `show` and
  `state`.
- **Hooks run only as approved.** By default only hooks the user approved
  run; an unapproved one stops the operation, and the agent is told to ask
  the user, who approves them in the view (it lists the exact commands and
  asks twice). The **Worktrunk hooks** setting can instead skip hooks
  (`--no-hooks`) or run all (`--yes`); the last is the user's choice
  alone.
- **Prompts never hang.** stdin is closed, so anything that would ask
  fails at once with its message.
- **A chat keeps its folder.** Removing or merging away the chat's own
  worktree needs an explicit argument.
- **Powers.** A process plugin with tools runs programs and gives agents
  tools (which includes starting chats); the plugin asks only for
  `network` beyond that, for release checks and downloads. It reads no
  chats, holds no secrets, and every tool call goes through the chat's
  own approval. Its install record is what counts: editing its files
  grants nothing.

## Settings

| Setting | Default | |
|---|---|---|
| Worktrunk program | empty | Full path of `wt`/`git-wt`. Empty: the plugin's copy, else found. |
| Worktrunk hooks | Run hooks I approved | Or Never run hooks, or Run every hook without asking. |
| Harness for new chats | empty | An agent's new chat uses the asking chat's harness; the view's uses `claude`. |
| Check for Worktrunk updates | on | Every few hours, from Worktrunk's tags. |

## Building and installing

`build.sh` builds `bin/worktrunk-plugin` (Rust, its own workspace; two
small crates). Then Plugins, install from a folder, or from the catalog.
The program's tests (`cargo test` in `program/`) run the program over
stdio against a fake `wt` that records every command line; set
`WORKTRUNK_TEST_BIN` to a real Worktrunk to also run it for real. The
daemon's `e2e_plugin_examples` installs it from a copy of the catalog.

## What OpenRemote gained for it

- **A process plugin's pages run in the console**: its sidebar views and
  composer buttons, in the same sandboxed frame as a JavaScript plugin's,
  reaching its program through `openremote.program.request(name, params)`
  (`POST /plugins/{id}/request`, `ui/request` to the program). Chats and
  the network stay with the program, which the daemon gates
  ([js-api.md](js-api.md)).
- **A tool call says which harness and model the asking chat runs**
  (`chat.harness`, `chat.model`), so a tool can start a chat like it
  ([protocol.md](protocol.md)).
- **Every chat shows its git branch**, and a linked worktree says whose:
  `POST /git/places`, read from git's own files, this device's console
  only.
