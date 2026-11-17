# Reusing connectors in OpenRemote

Researched 2026-11-15. Primary sources only. Public Codex source inspected at [`ee344ca9458351a6ea102795fa73110ad3ede5dc`](https://github.com/openai/codex/tree/ee344ca9458351a6ea102795fa73110ad3ede5dc), committed today. Installed CLI: `codex-cli 0.167.0`. No user configuration, credential store, OAuth tokens, or provider data inspected; no login or hosted connector calls made.

## Recommendation

Build one connector manager around existing MCP servers, with optional adapters to the Apps already installed on the computer. Avoid implementing Gmail, Notion, Slack, etc. APIs individually. Codex is a particularly promising adapter: its app-server already exposes catalog discovery, installed state, tool schemas, and direct tool calls. However, a local direct-call proof does not establish that all ChatGPT connectors may be redistributed as a universal gateway to other models.

Start with portable vendor MCP connections and native connector use inside each App. Treat cross-model access through Codex as a focused compatibility experiment, pending a real hosted-app read test, permission handling, and confirmation of supported service use.

## What Codex actually exposes

| Surface | What it gives OpenRemote |
| --- | --- |
| `app/list` | Paginated connector directory merged with accessible apps; `installUrl`, `isAccessible`, `isEnabled`. |
| `app/read` | Metadata and optional display-only tool summaries, not callable schemas or authorization. |
| `app/installed` | Effective installed runtime `enabled` and `callable` state; `forceRefresh` refreshes the hosted snapshot. |
| `mcpServerStatus/list` | MCP server status, authentication state, and actual tool schemas; supply the loaded `threadId`. |
| `mcpServer/tool/call` | Direct call using `threadId`, `server`, `tool`, optional `arguments` and `_meta`. |

These are current documented client APIs. App mentions through `turn/start` are the normal model-driven route. Direct tool calls are a separate client route. [App-server documentation](https://learn.chatgpt.com/docs/app-server), [app protocol](https://github.com/openai/codex/blob/ee344ca9458351a6ea102795fa73110ad3ede5dc/codex-rs/app-server-protocol/src/protocol/v2/apps.rs#L10-L65), [direct-call parameters](https://github.com/openai/codex/blob/ee344ca9458351a6ea102795fa73110ad3ede5dc/codex-rs/app-server-protocol/src/protocol/v2/mcp.rs#L218-L247).

Local verification: generated 0.167.0 schemas in ignored `.scratch/codex-connector-protocol-0167` contain these methods. A separate isolated probe used a temporary `CODEX_HOME`, an ephemeral thread, a fake MCP echo server, and a local model endpoint that counts requests. Discovery and `mcpServer/tool/call` returned the echo; model requests: **zero**. Evidence: ignored `.scratch/probe-codex-connectors.mjs` and `.scratch/codex-connector-probe-result.json`. This proves direct arbitrary-MCP transport on the installed binary, not hosted-app entitlement.

The source agrees: the client handler loads a thread and calls `thread.call_mcp_tool`; that refreshes MCP and invokes `latest_call_tool_without_attribution`. It does not start a model turn. Therefore an OpenRemote MCP adapter could technically expose discovered schemas to Claude or another model and dispatch their tool calls through Codex. This is an architectural inference, not an advertised cross-model connector service. [Handler](https://github.com/openai/codex/blob/ee344ca9458351a6ea102795fa73110ad3ede5dc/codex-rs/app-server/src/request_processors/mcp_processor.rs#L796-L826), [thread call](https://github.com/openai/codex/blob/ee344ca9458351a6ea102795fa73110ad3ede5dc/codex-rs/core/src/codex_thread.rs#L1517-L1533), [runtime dispatch](https://github.com/openai/codex/blob/ee344ca9458351a6ea102795fa73110ad3ede5dc/codex-rs/codex-mcp/src/runtime.rs#L856-L894).

Important implementation constraint: direct dispatch checks the MCP server/tool filter but does not enter Codex's model-driven app policy and approval evaluator. Do not assume model approval prompts protect bridge calls. OpenRemote must enforce its own allowed-app/tool set, managed restrictions, and user authorization for actions before dispatch. [Direct validation](https://github.com/openai/codex/blob/ee344ca9458351a6ea102795fa73110ad3ede5dc/codex-rs/codex-mcp/src/connection_manager.rs#L1496-L1506), [separate model approval evaluator](https://github.com/openai/codex/blob/ee344ca9458351a6ea102795fa73110ad3ede5dc/codex-rs/core/src/mcp_tool_call.rs#L2120-L2167).

## Hosted apps, authentication, and subscription limits

Codex's hosted connector code is a client for ChatGPT infrastructure, not open-source implementations of every integration. It constructs a host-owned `codex_apps` HTTP MCP connection with ChatGPT authentication, a backend `/ps/mcp` URL, and product SKU `codex`. Its connector catalog is also fetched remotely. Keep these backend details behind Codex rather than copying private endpoint/auth logic. [Hosted connection](https://github.com/openai/codex/blob/ee344ca9458351a6ea102795fa73110ad3ede5dc/codex-rs/codex-mcp/src/mcp/mod.rs#L660-L713), [directory loading](https://github.com/openai/codex/blob/ee344ca9458351a6ea102795fa73110ad3ede5dc/codex-rs/connectors/src/lib.rs#L263-L292).

The gate is `features.apps`, currently default-on, plus Codex-backend authentication. Direct API-key and Bedrock auth modes are excluded. Backend auth also includes managed modes, so describing the check as exclusively an interactive ChatGPT login would be imprecise. Local controls are `apps._default.enabled`, `apps.<id>.enabled`, and per-tool settings; `apps.enabled` is not the documented global switch. [Feature](https://github.com/openai/codex/blob/ee344ca9458351a6ea102795fa73110ad3ede5dc/codex-rs/features/src/lib.rs#L1739-L1744), [hosted gate](https://github.com/openai/codex/blob/ee344ca9458351a6ea102795fa73110ad3ede5dc/codex-rs/codex-mcp/src/mcp/mod.rs#L396-L398), [auth modes](https://github.com/openai/codex/blob/ee344ca9458351a6ea102795fa73110ad3ede5dc/codex-rs/app-server-protocol/src/protocol/common.rs#L64-L89), [configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference).

Use the returned install URL or native plugin flow for hosted app setup, then refresh discovery. Installation, provider authorization, and workspace permission are separate. `mcpServer/oauth/login` is the flow for a configured MCP server, not a universal account-connect endpoint for ChatGPT apps. [App setup](https://help.openai.com/en/articles/20001494-connecting-and-managing-app-accounts-in-chatgpt), [app-server OAuth](https://learn.chatgpt.com/docs/app-server).

An existing ChatGPT subscription enables the eligible Codex/Work experience, with shared usage limits. Connector availability also depends on plan, region, workspace, role, model, and interface; third-party account permissions still apply. Public docs reviewed do not promise an unlimited external connector gateway, specify standalone direct-connector billing, or guarantee hosted connectors for arbitrary third-party models. Preserve that uncertainty. [Pricing](https://learn.chatgpt.com/docs/pricing), [app availability](https://help.openai.com/en/articles/11487775-connected-apps-in-chatgpt).

Codex CLI source is Apache-2.0. Source reuse permission should not be interpreted as entitlement to hosted backend services. Older advice to run `codex mcp-server` is also unsuitable here: installed 0.167.0 does not recognize that subcommand. The current app-server is the relevant interface. [Pinned license](https://github.com/openai/codex/blob/ee344ca9458351a6ea102795fa73110ad3ede5dc/LICENSE).

## Portable reuse and other catalogs

MCP provides a reusable connection layer, not transferable sign-ins. Its HTTP authorization standard uses OAuth, protected-resource discovery, audience-bound tokens, and current Client ID Metadata Documents; DCR remains for compatibility. Tokens issued to another resource/client cannot simply be copied from another app's store. Local stdio servers normally obtain credentials through their environment. [MCP 2026-07-28 authorization](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization).

Existing vendor servers provide service functionality without OpenRemote writing it:

- **Notion:** official remote URL, OAuth, and setup instructions for Codex, Claude Code, Pi, and other clients. [Notion setup](https://developers.notion.com/guides/mcp/get-started-with-mcp).
- **GitHub:** official remote and local MCP server; custom hosts need their own OAuth client registration or supported PAT setup. [GitHub server](https://github.com/github/github-mcp-server#remote-github-mcp-server).
- **Google Workspace:** official MCP servers exist, but currently require Developer Preview access and Google Cloud setup. Do not present them as universal one-click connections. [Google prerequisites](https://developers.google.com/workspace/guides/configure-mcp-servers).

Claude Code already imports claude.ai connectors for eligible subscription logins. Some Anthropic-hosted connectors only accept claude.ai's registered OAuth redirect and cannot be connected through local OAuth. Reusing the native Claude route is supported; exporting all those hosted connections to other models is not established by that support. [Claude connector behavior](https://code.claude.com/docs/en/mcp#use-mcp-servers-from-claudeai).

The official MCP Registry supplies public server metadata, package/remote locations, and installation information. It is not a user's connected-app catalog or authorization broker. Its guidance recommends downstream curated registries for host applications. Use a small verified vendor catalog initially, then a registry adapter if useful. [Registry purpose](https://modelcontextprotocol.io/registry/about), [aggregator API](https://modelcontextprotocol.io/registry/registry-aggregators).

## Next proof and remaining blockers

With an explicitly selected connected hosted app, verify discovery and one read-only direct call through the user's Codex instance. Check account selection, reconnect errors, tool refresh, disabled-app behavior, and the bridge's own permission gate. Separately establish OpenAI's supported usage boundary for third-party model dispatch. Only then promise shared Codex-backed connectors. Portable MCP does not depend on that outcome.

## Where this fits OpenRemote

Repository inspection on the working branch found an existing path worth extending:

- `openremote-harness::McpServer` carries only `id`, `command`, and `args`. `McpServerEntry` persists a launch command and a key-acknowledgment flag, without remote endpoint or OAuth connection management. See `daemon/crates/openremote-harness/src/lib.rs`, `daemon/crates/openremote-core/src/model.rs`, and `src/AddMcpServerModal.jsx`.
- `supervisor/composer.rs::mcp_servers_for` already injects OpenRemote's per-chat `mcp-chat` server. `mcp_chat.rs` lists and dispatches browser, agent, and plugin tools through authenticated loopback calls. A connector module can use that existing path while enforcing connection and tool access for the calling chat.
- `registry.rs::SessionDriver::rides_mcp` currently supports Claude, Codex, and qualifying plugin-provided Apps. The other four built-in Apps cannot use a connector merely because it appears in the interface. Their drivers need real tool integration before shared access can be advertised.

Proposed implementation order, not an approved product specification: first prove one authenticated connection end to end, including permission checks and reconnect behavior; then introduce the common connector module and expose working connections in the composer. Use the existing `dev:mock` backend for sample connection states in the real UI with HMR. Native authentication, runtime dispatch, and device routing still require real integration checks. Account credentials stay with the executing computer or their owning runtime, rather than being copied into the browser or Cloud sync.
