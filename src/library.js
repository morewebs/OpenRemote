// Catalogs are not saved. An installation or a rule is created from one of these.

export const PLUGINS = [
  { id: 'github', name: 'GitHub', detail: 'Issues, pull requests, and checks.', needsKey: false, command: 'npx -y @modelcontextprotocol/server-github' },
  { id: 'gitlab', name: 'GitLab', detail: 'Merge requests and pipelines.', needsKey: true, command: 'npx -y @modelcontextprotocol/server-gitlab' },
  { id: 'linear', name: 'Linear', detail: 'Issues and project status.', needsKey: true, command: 'npx -y @modelcontextprotocol/server-linear' },
  { id: 'sentry', name: 'Sentry', detail: 'Production errors.', needsKey: true, command: 'npx -y @sentry/mcp-server' },
  { id: 'postgres', name: 'Postgres', detail: 'Read-only SQL against the app database.', needsKey: true, command: 'npx -y @modelcontextprotocol/server-postgres' },
  { id: 'sqlite', name: 'SQLite', detail: 'Query a database file on that machine.', needsKey: false, command: 'uvx mcp-server-sqlite' },
  { id: 'browser', name: 'Browser', detail: 'Open local pages from a session.', needsKey: false, command: 'npx -y @playwright/mcp' },
]

export const AUTOMATION_STARTERS = [
  {
    id: 'deps',
    name: 'Nightly dependency audit',
    trigger: { kind: 'schedule', time: '09:00', project: 'webapp' },
    harness: 'cc',
    model: 'sonnet',
    modelLabel: 'Sonnet 5.5',
    deviceId: 'mac-mini',
    project: 'webapp',
    task: 'Check outdated dependencies and list the ones that are safe to bump.',
  },
  {
    id: 'review',
    name: 'Review requested',
    trigger: { kind: 'review', project: 'webapp' },
    harness: 'cx',
    model: 'gpt6',
    modelLabel: 'GPT-6',
    deviceId: 'mac-mini',
    project: 'webapp',
    task: 'Read the diff and comment on anything that changes behavior.',
  },
  {
    id: 'sentry-issue',
    name: 'New Sentry issue',
    trigger: { kind: 'errors', project: 'api' },
    harness: 'od',
    model: 'auto',
    modelLabel: 'Auto',
    deviceId: 'server',
    project: 'api',
    task: 'Read the stack and find the throw.',
  },
  {
    id: 'release-tag',
    name: 'Release tag',
    trigger: { kind: 'release', project: 'site' },
    harness: 'cc',
    model: 'sonnet',
    modelLabel: 'Sonnet 5.5',
    deviceId: 'mac-mini',
    project: 'site',
    task: 'Draft the release notes from the commits since the previous tag.',
  },
]
