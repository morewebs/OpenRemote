// Probe: does `codex app-server` accept an mcp_servers config override
// on thread/start? Dotted keyPath form first, nested form as fallback.
import { spawn } from 'node:child_process'

const child = spawn(process.execPath, [
  process.env.APPDATA + '\\npm\\node_modules\\@openai\\codex\\bin\\codex.js',
  'app-server',
  '--listen',
  'stdio://',
], { stdio: ['pipe', 'pipe', 'pipe'] })
let nextId = 1
const pending = new Map()
const timeout = setTimeout(() => {
  console.log('PROBE TIMED OUT')
  child.kill()
  process.exit(1)
}, 25000)

const send = (method, params) =>
  new Promise((resolve) => {
    const id = nextId++
    pending.set(id, resolve)
    child.stdin.write(JSON.stringify({ id, method, params }) + '\n')
  })
const notify = (method, params) => child.stdin.write(JSON.stringify({ method, params }) + '\n')

let buffer = ''
child.stdout.on('data', (chunk) => {
  buffer += chunk
  let index
  while ((index = buffer.indexOf('\n')) >= 0) {
    const line = buffer.slice(0, index)
    buffer = buffer.slice(index + 1)
    if (!line.trim()) continue
    try {
      const m = JSON.parse(line)
      if (m.id != null && pending.has(m.id)) {
        pending.get(m.id)(m)
        pending.delete(m.id)
      } else if (m.method === 'commandExecution/requestApproval') {
        // answer nothing; not needed for this probe
      }
    } catch {
      /* not json */
    }
  }
})
child.stderr.on('data', (c) => process.stderr.write('[codex] ' + c))

const fake = new URL('./fake-mcp.mjs', import.meta.url).pathname.replace(/^\/(\w:)/, '$1')

const init = await send('initialize', {
  clientInfo: { name: 'or-probe', version: '0.0.1' },
  capabilities: { experimentalApi: true },
})
console.log('initialize:', init.error ? 'ERROR ' + JSON.stringify(init.error) : 'ok')
notify('initialized', {})

// Form 1: dotted keyPath keys.
const dotted = await send('thread/start', {
  cwd: process.cwd(),
  approvalPolicy: 'on-request',
  sandbox: 'workspace-write',
  config: {
    'mcp_servers.orprobe.command': 'node',
    'mcp_servers.orprobe.args': [fake],
  },
})
if (dotted.error) {
  console.log('thread/start dotted config: ERROR', JSON.stringify(dotted.error))
} else {
  console.log('thread/start dotted config: ok, thread', dotted.result?.thread?.id ?? '?')
}

// Whatever started: list MCP server status.
const status = await send('mcpServerStatus/list', { threadId: dotted.result?.thread?.id })
const servers = status.result?.data ?? []
console.log('mcpServerStatus/list:', status.error ? 'ERROR ' + JSON.stringify(status.error).slice(0, 200) : 'ok')
console.log(
  'servers:',
  JSON.stringify(servers.map((s) => s.name + ':' + (s.runtimeStatus ?? '?'))),
)
console.log('orprobe present:', servers.some((s) => s.name === 'orprobe'))

// Form 2 (only if dotted failed): nested object.
if (dotted.error) {
  const nested = await send('thread/start', {
    cwd: process.cwd(),
    approvalPolicy: 'on-request',
    sandbox: 'workspace-write',
    config: {
      mcp_servers: {
        orprobe: { command: 'node', args: [fake] },
      },
    },
  })
  console.log(
    'thread/start nested config:',
    nested.error ? 'ERROR ' + JSON.stringify(nested.error) : 'ok, thread ' + (nested.result?.thread?.id ?? '?'),
  )
}

clearTimeout(timeout)
child.kill()
process.exit(0)
