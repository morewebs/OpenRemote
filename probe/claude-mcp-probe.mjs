// Probe: does real Claude Code accept --mcp-config as an argv JSON string?
// Zero tokens: we never send a prompt — MCP servers initialize at boot, a
// broken one logs to stderr. Alive+quiet = accepted; a missing command
// must complain (the difference proves the config rides).
import { spawn } from 'node:child_process'

const fake = new URL('./fake-mcp.mjs', import.meta.url).pathname.replace(/^\/(\w:)/, '$1')

const run = (label, servers) =>
  new Promise((resolve) => {
    const config = JSON.stringify({ mcpServers: servers })
    const child = spawn(process.env.APPDATA + '\\npm\\node_modules\\@anthropic-ai\\claude-code\\bin\\claude.exe', [
      '--output-format', 'stream-json',
      '--verbose',
      '--input-format', 'stream-json',
      '--permission-prompt-tool', 'stdio',
      '--mcp-config', config,
    ], { stdio: ['pipe', 'pipe', 'pipe'] })
    let out = ''
    let err = ''
    child.stdout.on('data', (c) => (out += c))
    child.stderr.on('data', (c) => (err += c))
    setTimeout(() => {
      const alive = child.exitCode === null
      console.log(label, '| alive:', alive, '| stderr:', err.trim().slice(0, 140) || '(clean)')
      child.kill()
      resolve()
    }, 5000)
  })

await run('valid server ', { orprobe: { command: 'node', args: [fake] } })
await run('broken server', { orprobe: { command: 'definitely-missing-binary-xyz', args: [] } })
process.exit(0)
