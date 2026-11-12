# Worktrunk for OpenRemote

Git worktrees for your agents, through [Worktrunk](https://worktrunk.dev)
(`wt`). OpenRemote's official Worktrunk plugin.

- **Agents** list, create, switch, merge and remove worktrees, and hand a
  piece of work to a new chat in its own worktree (`worktrunk__start_chat`).
- **You** get a Worktrees view (Plugins in the sidebar, or the Worktrees
  button in any composer): your repositories, their worktrees, a new
  worktree with or without a chat, hook approvals, and Worktrunk's version
  with one-click install and update.
- **Every chat** shows its branch, and whose worktree it is.

It follows Worktrunk's releases on its own: what each version can do is
read from that version's `--help`, unknown output passes through, and any
new command is reachable with `help` and `run`.

## Build and install

```sh
./build.sh
```

Then OpenRemote: Plugins, install from a folder, pick this folder (or
install Worktrunk from the catalog). Worktrunk itself is optional at
install: the view installs it if it's missing.

## Test

```sh
cd program && cargo test
WORKTRUNK_TEST_BIN=/path/to/wt cargo test   # also against a real Worktrunk
```

The design, the security rules it keeps and every setting are in
[docs/plugins/worktrunk.md](../../../docs/plugins/worktrunk.md).
