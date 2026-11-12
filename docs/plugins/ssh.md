# The SSH plugin

[`examples/plugins/ssh`](../../examples/plugins/ssh) gives a chat's agent
your own servers: hosts that run no OpenRemote, reached through this
computer's ssh with your keys. It is a process plugin
([authoring.md](authoring.md)): one compiled Rust program, `program/`,
built by `build.sh` into `bin/openremote-ssh` (`bin/openremote-ssh.exe` on
Windows), then installed from Plugins > Install from a folder.

It is not in OpenRemote's catalog: a catalog entry installs its folder as
it is, and this folder holds no program until `build.sh` has run.

## What an agent gets

| Tool | Does |
|---|---|
| `ssh__ssh_hosts` | Lists the hosts by name, address and whether the host key is verified. |
| `ssh__ssh_run` | Runs one command on a host's login shell: exit code, stdout and stderr. 60 s by default, 300 s at most; 200,000 characters of output, keeping the end. |
| `ssh__ssh_read_file` | The start of a text file, up to 200,000 bytes. Binary files are refused. |
| `ssh__ssh_write_file` | Replaces or appends to a text file, up to 2,000,000 bytes. |

Every call is approved like any other agent tool: the chat's permission
mode decides whether you are asked first. `ssh_run` sends a command to a
shell on the host, so it can do whatever that account can do there; pick
the account each host signs in as with that in mind. `ssh_read_file` runs
a command in that shell too (`head`), so only `ssh_hosts` is marked
read-only.

Each host's login shell must be a POSIX shell (sh, bash, dash, zsh, ksh):
paths are quoted for one, and `ssh_read_file` and `ssh_write_file` run
`head -c` and `cat`. A host whose account logs in to fish, csh or
PowerShell gets `ssh_run` only, with commands written for that shell.

Every call ends before the daemon stops waiting for it, so a call the
agent is told timed out is not still running: a call waits at most 10 s
for one of the four connections (and is refused, never started later, if
none frees), then runs within its own limit, then has 5 s to stop. Those
three together stay at least 10 s under the tool's `timeout_secs`, and a
test checks the sum for every tool. The page's requests wait 5 s for a
connection and run 25 s, inside the 60 s the daemon gives them.

## Settings

All four are plain text, and none is a secret:

| Setting | |
|---|---|
| Hosts | `name=user@address` or `name=user@address:port`, separated by commas: `web-1=deploy@203.0.113.5, db=root@db.example.com:2222, v6=me@[2001:db8::1]`. A name is lowercase letters, digits and single dashes; a user is a plain account name; an address is a host name or an IP address. An entry of any other shape is left out, and the plugin's page and `ssh_hosts` say which and why without repeating it. |
| Key file | Optional: the path of a private key, like `~/.ssh/id_ed25519`. |
| ssh-agent socket | Optional: the path your terminal's `SSH_AUTH_SOCK` holds (macOS, Linux), or on Windows `\\.\pipe\openssh-ssh-agent` to use the OpenSSH agent service. Left empty, no agent's keys are offered. |
| Path to ssh | Optional: the full path of the ssh program. |

## What it will not do

Each of these is pinned by a test in `program/` (`cargo test` there; the
system's ssh is played by `program/examples/fake_ssh.rs`, which records
every argument and variable it was started with).

- **Keep a key or a password.** There is no secret setting and no field
  for a password. A key is a file's path, and a value that looks like a key
  (a `-----BEGIN` block, an `ssh-ed25519 AAAA...` line, a PuTTY key) typed
  into Key file is refused and never used; a `user:password@host` entry is
  left out. OpenRemote has already saved whatever was typed into a setting,
  so the refusal says to clear it, and it never repeats the value. A key
  with a passphrase is used through ssh-agent (`ssh-add` it first), since
  nothing here can ask for the passphrase.
- **Turn off host-key checking, or accept a changed key.** Every real
  connection runs with `StrictHostKeyChecking=yes` and `UpdateHostKeys=no`
  against two known_hosts files: the plugin's own (in its data folder) and
  yours (`~/.ssh/known_hosts`). A connection never writes either. A host
  whose key is in neither is refused by ssh before it signs in or sends anything; a host whose
  key does not match is refused with words that say so and how to check
  (the page's Verify, or replacing the old key from your own terminal),
  and the call is not retried.
- **Let an agent's tools trust a key.** Keys are verified by you: connect once
  from your own terminal and accept the key there, or use the plugin's page
  (below). Verify looks at the key with an empty scratch file, offers no
  key and no agent (`PreferredAuthentications=none`, `IdentityAgent=none`),
  shows the fingerprint, and records the key only when you confirm that
  same fingerprint within five minutes, for the host as its settings are
  then. The page's requests are not tools, so a chat's agent can't call
  them. They are the daemon's `POST /plugins/{id}/request`, which takes
  the daemon's token: like every console route, any program on this
  computer that holds that token (it is in an owner-only file) could drive
  Verify, as it could drive the rest of the console. The line drawn here is
  between a chat's agent and you, not between programs running as you.
- **Take ssh options from anyone.** ssh starts with `-F none`, so no ssh
  config is read and nothing in one can add a `ProxyCommand`, a
  `LocalCommand`, a forward or a shared connection; `ProxyCommand=none` and
  `PermitLocalCommand=no` are said on the command line too. Every `-o` is a
  constant of `program/src/ssh.rs`, except the known_hosts paths. The
  host's user, port and address are each one argument (`-l`, `-p`, then the
  address after `--`), checked to a shape that can't begin with a dash or
  hold a space, and the remote command is one argument after the address.
  ssh is started directly, with no shell on this computer.
- **Quote a path wrongly.** A path for `ssh_read_file` or `ssh_write_file`
  is one single-quoted word for the remote shell; a leading `~/` becomes
  `"$HOME"/`, written by the plugin.
- **Find ssh on PATH.** OpenRemote gives a process plugin a cleared
  environment, and a program found on PATH is not the user's choice. ssh
  is the Path to ssh setting (a full path) or the first of
  `/usr/bin/ssh`, `/usr/local/bin/ssh`, `/opt/homebrew/bin/ssh`,
  `/bin/ssh`, or on Windows `%SystemRoot%\System32\OpenSSH\ssh.exe`,
  `%ProgramFiles%\OpenSSH\ssh.exe` and Git's `usr\bin\ssh.exe`.
- **Ask for more than it uses.** `powers` is empty. The install record
  holds what a process plugin with tools always has, `run_programs` and
  `agent_tools`, and nothing else: no `network` (ssh is a program, not an
  OpenRemote request) and no chats. Like every process plugin it runs as
  you, not sandboxed, and the install screen says so.

## ssh-agent and your home folder

OpenRemote starts the plugin with a cleared environment
([protocol.md](protocol.md)): `HOME` and, on Windows, `USERPROFILE` are
passed, `SSH_AUTH_SOCK` is not. The plugin then starts ssh with an
environment of its own, cleared again and given only:

- what ssh needs to find your home folder (`~/.ssh/known_hosts`, the keys
  ssh tries by default) and to start: `HOME`, `USER`, `LOGNAME`, `TMPDIR`,
  `LANG`, `LC_ALL`, `LC_CTYPE`, and on Windows `USERPROFILE`, `USERNAME`,
  `HOMEDRIVE`, `HOMEPATH`, `SystemRoot`, `SYSTEMDRIVE`, `windir`, `TEMP`,
  `TMP`, `ProgramData` (Windows' ssh exits at once without it);
- `SSH_AUTH_SOCK`, only when you set the ssh-agent socket setting, and
  then to exactly that path.

Not `PATH`, `SSH_ASKPASS` or `DISPLAY`: nothing may prompt, and with no
config ssh runs no other program. The agent's socket is a setting rather
than something guessed because the agent is a power over every key it
holds; you name the one this plugin may use. Windows' ssh reaches the
OpenSSH agent service by its fixed pipe without any variable, so when the
setting is empty the plugin also passes `IdentityAgent=none` there: the
service's keys are offered only once you name its pipe
(`\\.\pipe\openssh-ssh-agent`) in the setting.

## The page

`ui/ssh.html` is the plugin's view (`place: machines`): your hosts, each
with its key's state, and Verify, Test and Forget key. It asks the
program through `POST /plugins/{id}/request` (`ui/request`: `state`,
`verify_start`, `verify_confirm`, `host_test`, `forget`).

Today the console shows only a JavaScript plugin's sidebar views
([authoring.md](authoring.md), "views"), so this page has no place to
render yet, and the frame's bridge has no call to a process plugin's
program. The page says so if it is opened. Until then, verify a host by
connecting to it once from your own terminal, and the tools use the key
you accepted there.
