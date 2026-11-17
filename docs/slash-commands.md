# Every App's slash commands

What each App offers after `/`, and how OpenRemote runs each one when a chat
here sends it. The built-in tables below are generated from
`daemon/crates/openremote-daemon/src/slash_tables.rs`, which is what the
daemon serves; the design is
`docs/superpowers/specs/2026-11-12-slash-commands-design.md`.

Inventoried 2026-11-12 against these versions: Claude Code from its docs
(code.claude.com/docs/en/commands and the headless and Agent SDK pages; not
installed on the machine used), Codex CLI 0.167.0 (local binary and its
generated app-server schema, plus `codex-rs/tui/src/slash_command.rs` at
`rust-v0.167.0`), OpenCode 1.19.3 (local `opencode serve`: `/doc` and
`GET /command`), pi 1.4.0 (local source and `docs/rpc.md`), grok 1.0.77
(local binary and its embedded docs), Antigravity CLI 1.5.3 (local `agy`,
its `/help` in print mode and its changelog), and Freebuff 0.2.36 (local npm
package).

## How a command runs here

The "Here" column says which of four routes a command takes:

- **Sent to the App**: the message goes as typed and the App runs it.
- **App's own call**: OpenRemote calls the App's protocol for it, because
  sent as words it would reach the model as text (the "How" column names the
  call). The chat shows the command as typed, inside a normal turn.
- **OpenRemote**: the console does it itself (a picker, a view, a dialog, a
  new chat). If a phone or an automation sends the word as text, the daemon
  sends it on where the App runs it too (marked in "How"), and otherwise
  refuses it with the reason.
- **Refused**: shown dimmed in the `/` menu with its reason, refused in the
  composer before sending, and refused by the daemon (409) for any other
  client. No turn opens.

Every App also gets OpenRemote's own commands: `/model`, `/effort`,
`/fast`, `/permissions`, `/stop`, `/resume`, `/rename`, `/sync`, `/cloud`,
`/new`, `/help`, `/status`, `/copy`, `/export` and `/diff`. Where an App's
command has the same name, OpenRemote's runs.

Two rules decide between an App's built-in and a user's command of the same
name. Claude's own list holds its built-ins too, and Claude runs its
built-in first, so the table's route stands; but a row this table refuses
that Claude itself lists is sent, because Claude says it can run it
headless. Codex, OpenCode and pi list only custom commands at run time, so a
listed name there is the user's own and is always sent as typed.

## Custom commands, skills, plugins and MCP prompts

These are listed by each App at run time (`GET /commands`) and badged in the
menu: **custom** (a command file of the user's or the project's), **plugin**,
**MCP**, and the Skills group.

| App | Where they come from | How the list reaches OpenRemote | How they run |
|---|---|---|---|
| Claude Code | `~/.claude/commands/*.md`, `.claude/commands/*.md` (a sub-folder scopes the name, `folder:name`); skills in `~/.claude/skills/<name>/SKILL.md` and `.claude/skills`; plugins as `<plugin>:<name>`; MCP prompts as `mcp__<server>__<prompt>` | the `initialize` handshake's `commands` list, run in the chat's folder with the installed plugins; a running chat's `init` frame marks the list stale when it differs | sent as text; Claude expands `$ARGUMENTS`, `$1`, named arguments and `` !`cmd` `` itself |
| Codex | skills in `.agents/skills`, `~/.agents/skills`, `~/.codex/skills`; custom prompts (`~/.codex/prompts`, `/prompts:<name>`) are deprecated and absent from 0.167.0; MCP prompts do not become commands | `skills/list` on a short-lived app-server | the driver adds a `skill` input item for each `/name` that names a skill |
| OpenCode | `{command,commands}/**/*.md` in `~/.config/opencode/` and `.opencode/`; the `command` key of `opencode.json`; skills from `.claude/skills`, `.agents/skills` and OpenCode's own folders; MCP prompts (`source: "mcp"`) | `GET /command` on a short-lived `opencode serve` | `POST /session/:id/command`, so OpenCode fills the template |
| pi | prompt templates in `~/.pi/agent/prompts` and `.pi/prompts`; skills as `/skill:<name>`; extension commands (`pi.registerCommand`) | rpc `get_commands` | sent as text; pi expands templates and runs extensions |
| grok | `.grok/skills|commands`, `.claude/skills|commands`, `.agents/skills|commands`, `.cursor/skills`, in the folder, the repo and the home folder | not listed before a chat yet (see "Not done yet") | sent as text; headless grok runs skills, `commands/*.md` and `/workflow` |
| Antigravity | `.agents/skills`, `~/.gemini/config/skills`, plugins; workflows in `.agents/workflows` (deprecated) | not listed before a chat yet | sent as text; print mode expands skills and custom commands |
| Freebuff (a plugin App) | skills as `/skill:<name>` from `.agents/skills` or `.claude/skills` | its plugin does not declare `"commands": true`, so none | sent as typed into its terminal |

## Hot reload

The daemon watches the folders above (the workspace's and the home folder's,
for every workspace the composer has asked about in the last 30 minutes; a
folder that does not exist yet is watched through its nearest parent). A
change marks the cached lists stale after 300 ms of quiet and moves
`commands_rev`, which `/capabilities` carries on the console's 2.5 s beat;
the composer asks again when it moves, and every time the `/` menu opens, so
another machine's change shows the next time the menu opens there. Plugin
and MCP server changes move it too. `~/.claude.json` is not watched: Claude
rewrites it constantly.

## Claude Code

| Command | Aliases | Arguments | What it does | Here | How |
|---|---|---|---|---|---|
| /add-dir |  | <path> | Add a working folder to this session | Sent to the App |  |
| /advisor |  | [model\|off] | Turn the advisor model on or off | Sent to the App |  |
| /agents |  |  | Manage subagents | Refused | opens the terminal's agent editor; mention an agent with @ instead |
| /artifacts |  |  | List and attach artifacts | Refused | only works in the App's own terminal window |
| /auto-mode-setup |  |  | Draft auto mode settings | Refused | only works in the App's own terminal window |
| /autocompact |  | [auto\|<tokens>] | Set when the session compacts on its own | Sent to the App |  |
| /autofix-pr |  | [prompt] | Start a cloud session that fixes a pull request | Sent to the App |  |
| /background | /bg |  | Detach to a background agent | Refused | detaches the terminal session; this chat already runs in the background |
| /branch |  | [name] | Branch the conversation | Sent to the App |  |
| /btw |  | [question] | Ask a side question that stays out of the history | Sent to the App |  |
| /bug | /share |  | Report a bug | Refused | only works in the App's own terminal window |
| /cd |  |  | Move the session to another folder | Refused | moves the session to another folder; in OpenRemote, move the chat to another project from its menu |
| /chrome |  |  | Chrome integration settings | Refused | only works in the App's own terminal window |
| /clear | /reset, /new |  | Start a new conversation | OpenRemote | new chat, same App and folder (sent as text from a phone or automation) |
| /color |  | [color\|default] | Set the prompt bar color | Sent to the App |  |
| /compact |  | [instructions] | Summarize the conversation to free context | Sent to the App |  |
| /config | /settings | [key=value] | Open settings, or set keys | OpenRemote | Settings (sent as text from a phone or automation) |
| /context |  | [all] | Show how the context window is used | Sent to the App |  |
| /copy |  |  | Copy the last answer | OpenRemote | copy the last answer |
| /deep-research |  | <question> | Research a question in depth | Sent to the App |  |
| /design |  | [brief] | Make a design artboard | Sent to the App |  |
| /design-login |  |  | Sign in to Claude Design | Refused | changes the App's account or billing; do that from the App's own sign-in in Settings > Apps |
| /design-sync |  | [hint] | Upload a design system | Sent to the App |  |
| /desktop | /app |  | Continue in the desktop app | Refused | only works in the App's own terminal window |
| /diff |  |  | Show the changes in the working folder | OpenRemote | Changes view |
| /effort |  | [level\|auto] | Set how hard the model thinks | Sent to the App |  |
| /exit | /quit |  | Stop the chat | OpenRemote | stop the chat |
| /export |  | [filename] | Export the conversation | OpenRemote | copy the chat as Markdown |
| /fast |  | [on\|off] | Turn fast mode on or off | Sent to the App |  |
| /feedback |  |  | Send feedback | Refused | only works in the App's own terminal window |
| /focus |  |  | Focus view | Refused | only works in the App's own terminal window |
| /fork |  | [prompt] | Copy the conversation into a background session | Sent to the App |  |
| /goal |  | [condition\|clear] | Keep working until a condition is met | Sent to the App |  |
| /heapdump |  |  | Write a heap snapshot | Refused | only works in the App's own terminal window |
| /help |  |  | List every command | OpenRemote | /help dialog |
| /hooks |  |  | View hooks | Refused | opens the terminal's hook viewer; hooks live in .claude/settings.json |
| /ide |  |  | IDE integration | Refused | only works in the App's own terminal window |
| /import |  | [codex\|gemini\|cursor] | Import setup from another agent | Sent to the App |  |
| /init |  |  | Write a CLAUDE.md for this project | Sent to the App |  |
| /insights |  |  | Write a usage report | Sent to the App |  |
| /install-github-app |  |  | Set up the GitHub app | Refused | only works in the App's own terminal window |
| /install-slack-app |  |  | Set up the Slack app | Refused | only works in the App's own terminal window |
| /keybindings |  |  | Open the keybindings file | Refused | only works in the App's own terminal window |
| /list-agents | /peers |  | List the agents this one can message | Sent to the App |  |
| /login |  |  | Sign in | OpenRemote | Settings > Apps |
| /logout |  |  | Sign out | OpenRemote | Settings > Apps |
| /mcp |  | [reconnect\|enable\|disable <server>] | Show and manage MCP servers | OpenRemote | Settings > MCP servers (sent as text from a phone or automation) |
| /memory |  |  | Edit memory files | Refused | opens the terminal's memory editor; edit CLAUDE.md in the project instead |
| /mobile | /ios, /android |  | Show a QR code for the phone app | Refused | only works in the App's own terminal window |
| /model |  | [model] | Switch the model | OpenRemote | model picker (sent as text from a phone or automation) |
| /output-style |  | [style] | List or switch the output style | Sent to the App |  |
| /passes |  |  | Guest passes | Refused | changes the App's account or billing; do that from the App's own sign-in in Settings > Apps |
| /permissions | /allowed-tools |  | Set the permission mode | OpenRemote | permission picker |
| /plan |  | [description] | Enter plan mode | Sent to the App |  |
| /plugin |  | [subcommand] | Manage plugins | OpenRemote | Plugins view (sent as text from a phone or automation) |
| /powerup |  |  | Interactive lessons | Refused | only works in the App's own terminal window |
| /privacy-settings |  |  | Privacy settings | Refused | changes the App's account or billing; do that from the App's own sign-in in Settings > Apps |
| /radio |  |  | Lo-fi radio | Refused | only works in the App's own terminal window |
| /rate-limit-options |  |  | Usage limit options | Refused | changes the App's account or billing; do that from the App's own sign-in in Settings > Apps |
| /recap |  |  | Summarize the session in one line | Sent to the App |  |
| /release-notes |  |  | Release notes | Refused | only works in the App's own terminal window |
| /reload-plugins |  | [--force] | Reload plugins | Sent to the App |  |
| /reload-skills |  |  | Scan skills and commands again | Sent to the App |  |
| /remote-control | /rc |  | Expose this session to claude.ai | Refused | hands the terminal to a remote client; this chat already runs through OpenRemote |
| /remote-env |  |  | Default cloud environment | Refused | only works in the App's own terminal window |
| /rename |  | [name] | Rename the chat | OpenRemote | rename the chat (sent as text from a phone or automation) |
| /restart | /update |  | Restart to apply an update | Refused | only works in the App's own terminal window |
| /resume | /continue |  | Resume a stopped chat | OpenRemote | resume a stopped chat |
| /rewind | /checkpoint, /undo |  | Rewind code or conversation | Refused | opens the terminal's checkpoint picker |
| /sandbox |  |  | Turn the sandbox on or off | Sent to the App |  |
| /schedule | /routines | [description] | Cloud scheduled tasks | Sent to the App |  |
| /scroll-speed |  |  | Mouse wheel speed | Refused | only works in the App's own terminal window |
| /security-review |  |  | Review the branch for security issues | Sent to the App |  |
| /setup-bedrock |  |  | Bedrock setup | Refused | only works in the App's own terminal window |
| /setup-vertex |  |  | Vertex setup | Refused | only works in the App's own terminal window |
| /skill-doctor |  |  | Report what skills cost | Sent to the App |  |
| /skills |  |  | List skills | OpenRemote | /help dialog |
| /status |  |  | Show this chat's App, model and settings | OpenRemote | /status dialog |
| /statusline |  | [description] | Set up the status line | Sent to the App |  |
| /stickers |  |  | Order stickers | Refused | only works in the App's own terminal window |
| /stop |  |  | Stop the chat | OpenRemote | stop the chat |
| /subtask |  | <task> | Run a task in a background subagent | Sent to the App |  |
| /tasks | /bashes |  | Background work | Refused | opens the terminal's task list |
| /team-onboarding |  |  | Write an onboarding guide from history | Sent to the App |  |
| /teleport | /tp |  | Pull a cloud session here | Refused | only works in the App's own terminal window |
| /terminal-setup |  |  | Shift+Enter key binding | Refused | only works in the App's own terminal window |
| /theme |  |  | Color theme | Refused | only works in the App's own terminal window |
| /tui |  |  | Terminal renderer | Refused | only works in the App's own terminal window |
| /ultrareview |  | [PR\|branch] | Deep review in the cloud | Sent to the App |  |
| /upgrade |  |  | Upgrade the plan | Refused | changes the App's account or billing; do that from the App's own sign-in in Settings > Apps |
| /usage | /cost, /stats |  | Cost, limits and stats | Sent to the App |  |
| /usage-credits |  |  | Usage credits | Refused | changes the App's account or billing; do that from the App's own sign-in in Settings > Apps |
| /voice |  |  | Voice dictation | Refused | only works in the App's own terminal window |
| /web-setup |  |  | Connect GitHub for the cloud | Refused | only works in the App's own terminal window |
| /workflows |  |  | Workflow progress view | Refused | only works in the App's own terminal window |

Claude's bundled skills (`/code-review` and its alias `/review`, `/simplify`,
`/loop`, `/batch`, `/debug`, `/doctor`, `/run`, `/verify`, `/claude-api`,
`/dataviz`, `/update-config`, `/fewer-permission-prompts` and others) come
from Claude's own list and are sent as text. `/pr-comments`, `/vim` and
`/ultraplan` were removed from Claude Code and are not listed.

## Codex

No Codex built-in runs when sent as text: Codex's terminal parses them, and
the app-server passes a `/` message to the model as words.

| Command | Aliases | Arguments | What it does | Here | How |
|---|---|---|---|---|---|
| /model |  |  | Choose the model and reasoning effort | OpenRemote | model picker |
| /presets |  |  | Saved prompts | Refused | opens the terminal's preset dial; send the prompt itself |
| /daybreak |  |  | Turn Daybreak on or off | Refused | only works in the App's own terminal window |
| /ide |  |  | Include the IDE's selection | Refused | only works in the App's own terminal window |
| /permissions |  |  | Choose what Codex may do | OpenRemote | permission picker |
| /keymap |  |  | Remap shortcuts | Refused | only works in the App's own terminal window |
| /vim |  |  | Vim keys | Refused | only works in the App's own terminal window |
| /setup-default-sandbox |  |  | Set up the elevated sandbox | Refused | needs an elevated prompt in the terminal |
| /experimental |  |  | Experimental features | Refused | opens the terminal's feature list; set them in ~/.codex/config.toml |
| /approve |  |  | Approve a denied action once | Refused | answers the terminal's own review; approve from the chat's prompt instead |
| /memories |  |  | Configure memories | Refused | opens the terminal's memory panel |
| /skills |  |  | List skills | OpenRemote | /help dialog |
| /import |  |  | Import setup from Claude Code | Refused | only works in the App's own terminal window |
| /hooks |  |  | Lifecycle hooks | Refused | opens the terminal's hook viewer |
| /review |  | [instructions] | Review the current changes | App's own call | `review/start` |
| /rename |  | [name] | Rename the chat | OpenRemote | rename the chat |
| /new | /clear |  | Start a new chat | OpenRemote | new chat, same App and folder |
| /archive |  |  | Archive the session | Refused | is done on the chat in OpenRemote's sidebar |
| /delete |  |  | Delete the session | Refused | is done on the chat in OpenRemote's sidebar |
| /resume |  |  | Resume a stopped chat | OpenRemote | resume a stopped chat |
| /fork |  |  | Fork the chat | Refused | is how the terminal picks a saved conversation; OpenRemote keeps each one as a chat in the sidebar |
| /worktree |  |  | Start in a new worktree | Refused | moves the session to another folder; in OpenRemote, move the chat to another project from its menu |
| /app |  |  | Continue in the desktop app | Refused | only works in the App's own terminal window |
| /init |  |  | Write an AGENTS.md for this project | App's own call | OpenRemote's AGENTS.md prompt |
| /compact |  |  | Summarize the conversation to free context | App's own call | `thread/compact/start` |
| /config |  |  | Preferences | OpenRemote | Settings |
| /recap |  |  | Summarize the conversation now | Refused | has no call Codex offers outside its terminal; ask for a summary instead |
| /plan |  |  | Switch to plan mode | Refused | has no call OpenRemote can make on this Codex version yet |
| /voice |  |  | Voice | Refused | only works in the App's own terminal window |
| /goal |  | [objective\|clear] | Set, show or clear a long-running goal | App's own call | `thread/goal/get\|set\|clear` |
| /agents |  |  | Agent overview | Refused | opens the terminal's agent centre |
| /side | /btw |  | Side conversation | Refused | forks an ephemeral thread the terminal shows beside the chat |
| /copy |  |  | Copy the last answer | OpenRemote | copy the last answer |
| /export |  |  | Export the conversation | OpenRemote | copy the chat as Markdown |
| /raw |  |  | Raw scrollback | Refused | only works in the App's own terminal window |
| /tui |  |  | Terminal mode | Refused | only works in the App's own terminal window |
| /diff |  |  | Show the changes in the working folder | OpenRemote | Changes view |
| /mention |  |  | Mention a file | Refused | is the terminal's file picker; type @ in the composer instead |
| /status |  |  | Show this chat's App, model and settings | OpenRemote | /status dialog |
| /daemon |  |  | Background server | Refused | only works in the App's own terminal window |
| /warnings |  |  | Retained warnings | Refused | only works in the App's own terminal window |
| /cd |  |  | Change the working folder | Refused | moves the session to another folder; in OpenRemote, move the chat to another project from its menu |
| /pwd | /cwd |  | Show the working folder | Refused | is shown at the top of the chat |
| /usage |  |  | Usage and limits | OpenRemote | /status dialog |
| /debug-config |  |  | Config layers | Refused | only works in the App's own terminal window |
| /title |  |  | Terminal title | Refused | only works in the App's own terminal window |
| /statusline |  |  | Status line | Refused | only works in the App's own terminal window |
| /theme |  |  | Syntax theme | Refused | only works in the App's own terminal window |
| /pets | /pet |  | Terminal pet | Refused | only works in the App's own terminal window |
| /mcp |  |  | MCP servers | OpenRemote | Settings > MCP servers |
| /apps |  |  | Connectors | OpenRemote | Plugins view |
| /plugins |  |  | Plugins | OpenRemote | Plugins view |
| /logout |  |  | Sign out | OpenRemote | Settings > Apps |
| /quit | /exit |  | Stop the chat | OpenRemote | stop the chat |
| /feedback |  |  | Send logs to the maintainers | Refused | only works in the App's own terminal window |
| /ps |  |  | Background terminals | Refused | only works in the App's own terminal window |
| /stop | /clean |  | Stop the chat | OpenRemote | stop the chat |
| /subagents |  |  | Switch between subagents | Refused | only works in the App's own terminal window |
| /help |  |  | List every command | OpenRemote | /help dialog |

## OpenCode

`/init` and `/review` are OpenCode's own server-side commands and come from
its `GET /command` list; the rest are its terminal's, and run through its
server where a call exists.

| Command | Aliases | Arguments | What it does | Here | How |
|---|---|---|---|---|---|
| /init |  |  | Write an AGENTS.md for this project | Sent to the App |  |
| /review |  | [commit\|branch\|pr] | Review changes | Sent to the App |  |
| /sessions | /resume, /continue |  | Resume a stopped chat | OpenRemote | resume a stopped chat |
| /new | /clear |  | Start a new chat | OpenRemote | new chat, same App and folder |
| /models | /mo |  | Switch the model | OpenRemote | model picker |
| /variants |  |  | Switch the model variant | OpenRemote | effort picker |
| /agents |  |  | Switch agent | Refused | has no per-chat setting in OpenRemote yet; mention the agent with @ instead |
| /compact | /summarize |  | Summarize the conversation to free context | App's own call | `POST /api/session/:id/compact` |
| /undo |  |  | Undo the last message and its file changes | App's own call | `POST /session/:id/revert` |
| /redo |  |  | Bring back what /undo took away | App's own call | `POST /session/:id/unrevert` |
| /fork |  |  | Fork the session | Refused | is how the terminal picks a saved conversation; OpenRemote keeps each one as a chat in the sidebar |
| /rename |  | [name] | Rename the chat | OpenRemote | rename the chat |
| /timeline |  |  | Jump to a message | Refused | only works in the App's own terminal window |
| /copy |  |  | Copy the last answer | OpenRemote | copy the last answer |
| /export |  |  | Export the conversation | OpenRemote | copy the chat as Markdown |
| /thinking | /toggle-thinking |  | Show or hide reasoning | Refused | only works in the App's own terminal window |
| /timestamps | /toggle-timestamps |  | Show or hide timestamps | Refused | only works in the App's own terminal window |
| /editor |  |  | Write in an editor | Refused | only works in the App's own terminal window |
| /skills |  |  | List skills | OpenRemote | /help dialog |
| /mcps |  |  | MCP servers | OpenRemote | Settings > MCP servers |
| /connect |  |  | Add a provider | OpenRemote | Settings > Apps |
| /status |  |  | Show this chat's App, model and settings | OpenRemote | /status dialog |
| /diff |  |  | Show the changes in the working folder | OpenRemote | Changes view |
| /debug |  |  | Debug info | Refused | only works in the App's own terminal window |
| /themes |  |  | Theme | Refused | only works in the App's own terminal window |
| /help |  |  | List every command | OpenRemote | /help dialog |
| /update |  |  | Update OpenCode | Refused | is done from Settings > Apps |
| /exit | /quit, /q |  | Stop the chat | OpenRemote | stop the chat |
| /org | /orgs, /switch-org |  | Switch organisation | Refused | changes the App's account or billing; do that from the App's own sign-in in Settings > Apps |
| /workspaces |  |  | Workspaces | Refused | moves the session to another folder; in OpenRemote, move the chat to another project from its menu |
| /warp |  |  | Change workspace | Refused | moves the session to another folder; in OpenRemote, move the chat to another project from its menu |
| /move |  |  | Move the session | Refused | moves the session to another folder; in OpenRemote, move the chat to another project from its menu |

## pi

pi's built-ins are terminal-only in `--mode rpc`; sent as a prompt they
reach the model as words. Its templates, skills and extension commands run
as text.

| Command | Aliases | Arguments | What it does | Here | How |
|---|---|---|---|---|---|
| /settings |  |  | Settings menu | Refused | opens the App's own settings screen in its terminal |
| /model |  | [provider/model] | Pick a model | OpenRemote | model picker |
| /tree |  |  | Navigate the session tree | Refused | opens the terminal's session tree |
| /thinking |  | <level> | Set the thinking level | OpenRemote | effort picker |
| /scoped-models |  |  | Models for quick cycling | Refused | only works in the App's own terminal window |
| /export |  |  | Export the session | OpenRemote | copy the chat as Markdown |
| /import |  |  | Import a session from a file | Refused | is how the terminal picks a saved conversation; OpenRemote keeps each one as a chat in the sidebar |
| /share |  |  | Share the session | Refused | only works in the App's own terminal window |
| /bug |  |  | Report a bug | Refused | only works in the App's own terminal window |
| /copy |  |  | Copy the last answer | OpenRemote | copy the last answer |
| /name |  | <name> | Name the session | OpenRemote | rename the chat |
| /session |  |  | Show session info and stats | App's own call | rpc `get_session_stats` |
| /changelog |  |  | Changelog | Refused | only works in the App's own terminal window |
| /hotkeys |  |  | Keyboard shortcuts | Refused | only works in the App's own terminal window |
| /fork |  |  | Fork from an earlier message | Refused | is how the terminal picks a saved conversation; OpenRemote keeps each one as a chat in the sidebar |
| /clone |  |  | Duplicate the session | Refused | is how the terminal picks a saved conversation; OpenRemote keeps each one as a chat in the sidebar |
| /trust |  |  | Trust this project | Refused | is asked when the chat starts |
| /login |  |  | Sign in to a provider | OpenRemote | Settings > Apps |
| /logout |  |  | Sign out of a provider | OpenRemote | Settings > Apps |
| /new |  |  | Start a new chat | OpenRemote | new chat, same App and folder |
| /compact |  | [instructions] | Summarize the conversation to free context | App's own call | rpc `compact` |
| /resume |  |  | Resume a stopped chat | OpenRemote | resume a stopped chat |
| /reload |  |  | Reload extensions, skills and prompts | Refused | is done by OpenRemote: changed prompts and skills show in the / menu on their own, and a new chat loads changed extensions |
| /quit |  |  | Stop the chat | OpenRemote | stop the chat |

## grok

OpenRemote runs grok headless (`--prompt-file`), where only skills,
`commands/*.md` and `/workflow` run. Its terminal commands are refused,
except where OpenRemote has the same thing (model, effort, permissions,
copy, export, rename).

| Command | Aliases | Arguments | What it does | Here | How |
|---|---|---|---|---|---|
| /help |  |  | List every command | OpenRemote | /help dialog |
| /new | /clear |  | Start a new chat | OpenRemote | new chat, same App and folder |
| /resume | /load |  | Resume a stopped chat | OpenRemote | resume a stopped chat |
| /rewind | /undo |  | Rewind to an earlier turn | Refused | opens the terminal's turn picker |
| /compact |  |  | Compact the history | Refused | has no call grok offers outside its terminal; grok compacts on its own when the context fills |
| /compact-mode |  |  | Denser layout | Refused | only works in the App's own terminal window |
| /context |  |  | Context usage | Refused | is shown in the chat's context meter |
| /context-window |  |  | Set the context window | Refused | only works in the App's own terminal window |
| /model | /m |  | Switch the model | OpenRemote | model picker |
| /effort |  |  | Reasoning effort | OpenRemote | effort picker |
| /plan |  |  | Plan mode | OpenRemote | permission picker |
| /view-plan | /show-plan, /plan-view |  | Show the plan | Refused | only works in the App's own terminal window |
| /auto |  |  | Auto-review | OpenRemote | permission picker |
| /always-approve | /yolo |  | Skip permission prompts | OpenRemote | permission picker |
| /btw |  |  | Side question | Refused | needs a turn in flight to join; ask it as a message instead |
| /fork |  |  | Branch the session | Refused | is how the terminal picks a saved conversation; OpenRemote keeps each one as a chat in the sidebar |
| /cd |  |  | Change the working folder | Refused | moves the session to another folder; in OpenRemote, move the chat to another project from its menu |
| /copy |  |  | Copy the last answer | OpenRemote | copy the last answer |
| /export |  |  | Export the conversation | OpenRemote | copy the chat as Markdown |
| /transcript | /log |  | Transcript in a pager | Refused | only works in the App's own terminal window |
| /docs | /howto, /guides |  | How-to guides | Refused | only works in the App's own terminal window |
| /find |  |  | Search the scrollback | Refused | only works in the App's own terminal window |
| /jump |  |  | Jump to a turn | Refused | only works in the App's own terminal window |
| /expand |  |  | Show a collapsed block | Refused | only works in the App's own terminal window |
| /history |  |  | Prompt history | Refused | only works in the App's own terminal window |
| /edit-prompt |  |  | Write in an editor | Refused | only works in the App's own terminal window |
| /multiline | /ml |  | Swap Enter keys | Refused | only works in the App's own terminal window |
| /density |  |  | Tool-call density | Refused | only works in the App's own terminal window |
| /timestamps |  |  | Clock display | Refused | only works in the App's own terminal window |
| /theme | /t |  | Color theme | Refused | only works in the App's own terminal window |
| /vim-mode |  |  | Vim keys | Refused | only works in the App's own terminal window |
| /fullscreen | /minimal, /full |  | Render mode | Refused | only works in the App's own terminal window |
| /toggle-mouse-reporting |  |  | Mouse reporting | Refused | only works in the App's own terminal window |
| /timeline |  |  | Timeline sidebar | Refused | only works in the App's own terminal window |
| /tutorial | /tour, /onboarding |  | Tips | Refused | only works in the App's own terminal window |
| /home | /welcome |  | Welcome screen | Refused | only works in the App's own terminal window |
| /announcements |  |  | Announcements | Refused | only works in the App's own terminal window |
| /release-notes | /changelog |  | Release notes | Refused | only works in the App's own terminal window |
| /dashboard | /agents-dashboard, /sessions |  | Agent dashboard | Refused | only works in the App's own terminal window |
| /tasks |  |  | Background tasks | Refused | only works in the App's own terminal window |
| /queue |  |  | Queued prompts | Refused | is shown above the composer |
| /recap | /summarize |  | Summarize the session | Refused | has no call grok offers outside its terminal; ask for a summary instead |
| /session-info |  |  | Session info | OpenRemote | /status dialog |
| /usage | /cost |  | Usage | OpenRemote | /status dialog |
| /login |  |  | Sign in | OpenRemote | Settings > Apps |
| /logout |  |  | Sign out | OpenRemote | Settings > Apps |
| /privacy |  |  | Data sharing | Refused | changes the App's account or billing; do that from the App's own sign-in in Settings > Apps |
| /settings | /config, /preferences, /prefs |  | Settings | OpenRemote | Settings |
| /config-agents | /agents |  | Agent definitions | Refused | opens the App's own settings screen in its terminal |
| /personas |  |  | Personas | Refused | opens the App's own settings screen in its terminal |
| /skills |  |  | List skills | OpenRemote | /help dialog |
| /plugins | /plugin |  | Plugins | OpenRemote | Plugins view |
| /marketplace |  |  | Marketplace | OpenRemote | Plugins view |
| /hooks |  |  | View hooks | Refused | only works in the App's own terminal window |
| /mcps |  |  | MCP servers | OpenRemote | Settings > MCP servers |
| /memory | /mem |  | Browse memory | Refused | only works in the App's own terminal window |
| /remember |  |  | Save a memory note | Refused | has no headless form grok documents; ask grok to remember it instead |
| /flush |  |  | Save memory now | Refused | is unreliable outside grok's terminal, by grok's own docs |
| /dream |  |  | Consolidate memory | Refused | only works in the App's own terminal window |
| /loop |  |  | Recurring prompt | Refused | outlives a headless run; use Automations in OpenRemote instead |
| /workflows |  |  | Browse workflows | Refused | opens the terminal's browser; /workflow list answers here |
| /workflow |  | list\|runs\|stop\|<name> [args] | Run or list workflows | Sent to the App |  |
| /imagine |  |  | Generate an image | Refused | has no headless form grok documents |
| /imagine-video |  |  | Generate a video | Refused | has no headless form grok documents |
| /voice |  |  | Dictation | Refused | only works in the App's own terminal window |
| /feedback |  |  | Send feedback | Refused | only works in the App's own terminal window |
| /import-claude |  |  | Import Claude settings | Refused | only works in the App's own terminal window |
| /rename |  | <title> | Rename the chat | OpenRemote | rename the chat |
| /delete |  |  | Delete the session | Refused | is done on the chat in OpenRemote's sidebar |
| /doctor | /terminal-setup, /terminal-check, /terminal-info |  | Session and terminal check | Refused | only works in the App's own terminal window |
| /debug |  |  | Debug overlays | Refused | only works in the App's own terminal window |
| /quit | /exit |  | Stop the chat | OpenRemote | stop the chat |

## Antigravity

Antigravity's print mode answers twelve read-only commands locally with no
turn, refuses its interactive ones, and expands skills and custom commands.

| Command | Aliases | Arguments | What it does | Here | How |
|---|---|---|---|---|---|
| /help |  |  | List every command | OpenRemote | /help dialog |
| /changelog |  |  | Release notes | Sent to the App |  |
| /config | /settings |  | Settings | OpenRemote | Settings (sent as text from a phone or automation) |
| /credits |  |  | Remaining credits | Sent to the App |  |
| /usage | /quota |  | Model quota usage | Sent to the App |  |
| /model |  | [name] | Set the model | OpenRemote | model picker (sent as text from a phone or automation) |
| /effort |  |  | Reasoning effort | OpenRemote | effort picker (sent as text from a phone or automation) |
| /skills |  | [reload] | List skills | Sent to the App |  |
| /agents |  |  | List custom agents | Sent to the App |  |
| /hooks |  |  | Hook configuration | Sent to the App |  |
| /permissions |  |  | Tool permissions | OpenRemote | permission picker (sent as text from a phone or automation) |
| /plugin | /plugins | [list\|install\|uninstall] | Plugin manager | OpenRemote | Plugins view (sent as text from a phone or automation) |
| /compact |  |  | Summarize the history | Refused | is not run by Antigravity's print mode, which OpenRemote drives |
| /clear |  |  | Start a new chat | OpenRemote | new chat, same App and folder |
| /rewind |  |  | Rewind to an earlier message | Refused | has no turn history in Antigravity's print mode |
| /btw |  |  | Side question | Refused | has no turn in flight to join in Antigravity's print mode |
| /fork |  |  | Fork the conversation | Refused | is how the terminal picks a saved conversation; OpenRemote keeps each one as a chat in the sidebar |
| /resume | /conv, /conv-switch |  | Resume a stopped chat | OpenRemote | resume a stopped chat |
| /plan | /planning |  | Plan mode | OpenRemote | permission picker |
| /diff |  |  | Show the changes in the working folder | OpenRemote | Changes view |
| /artifact |  |  | Artifact viewer | Refused | only works in the App's own terminal window |
| /mcp |  |  | MCP servers | OpenRemote | Settings > MCP servers |
| /codesearch | /cs, /search |  | Code search | Refused | only works in the App's own terminal window |
| /open |  |  | File viewer | Refused | only works in the App's own terminal window |
| /context |  |  | Context breakdown | Refused | is shown in the chat's context meter |
| /keybindings |  |  | Key bindings | Refused | only works in the App's own terminal window |
| /feedback |  |  | Feedback form | Refused | only works in the App's own terminal window |
| /projects |  |  | Projects view | Refused | moves the session to another folder; in OpenRemote, move the chat to another project from its menu |
| /tasks |  |  | Task list | Refused | only works in the App's own terminal window |
| /title |  |  | Window title | Refused | only works in the App's own terminal window |
| /statusline |  |  | Status line | Refused | only works in the App's own terminal window |
| /add-dir |  |  | Add a folder | Refused | is set when the chat starts |
| /goal |  | <goal> | Set a persistent goal | Sent to the App |  |
| /browser |  |  | Browser subagent | Sent to the App |  |
| /voice |  |  | Dictation | Refused | only works in the App's own terminal window |
| /remote-control |  |  | Remote control | Refused | hands the terminal to a remote client; this chat already runs through OpenRemote |
| /copy |  |  | Copy the last answer | OpenRemote | copy the last answer |
| /teamwork-preview |  |  | Preview feature | Refused | only works in the App's own terminal window |
| /login |  |  | Sign in | OpenRemote | Settings > Apps |
| /logout |  |  | Sign out | OpenRemote | Settings > Apps |

## Freebuff

Freebuff is a plugin App: OpenRemote types into its real terminal, so every
command runs as a person typing it would, and an unknown `/x` shows "Command
not found" without reaching the model. Its plugin gives no command list, so
its commands are not in the `/` menu yet; OpenRemote's own commands are.
0.2.36 has `/help`, `/new`, `/history`, `/init`, `/logout`, `/exit`,
`/feedback`, `/usage` and `/skill:<name>`; the bare words `help`, `init`,
`new`, `logout` and `exit` also run as commands there.

## Not done yet, and not verified

- Claude Code was not installed on the machine used: its rows are from the
  docs. Claude's own list decides at run time, so a row the docs got wrong is
  corrected by what the CLI reports.
- Codex `/goal` answers through `thread/goal/*` as a note; whether a set goal
  starts Codex working on its own was not observed.
- OpenCode `/compact` uses the v2 `/api/session/:id/compact` route and is
  refused on a v1 server; `/undo` and `/redo` use the v1 `revert` and
  `unrevert` routes, which 1.19.3 still serves. None was run against a live
  session.
- grok and Antigravity list nothing before a chat starts. grok's headless
  `init` frame carries `slash_commands` and `skills`, and `grok inspect
  --json` lists skills with no model; neither is read yet.
- Antigravity: whether print mode's local answers apply to messages sent on
  `--input-format stream-json` (how OpenRemote sends) rather than only to an
  argv prompt, and whether `/compact` is refused there, is unverified; the
  table refuses `/compact`.
- Freebuff: a command that starts no model turn (`/help`, `/new`,
  `/history`) probably leaves its driver waiting until the stall timeout.
