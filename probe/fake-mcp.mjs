// A minimal stdio MCP server for the codex injection probe (throwaway).
import readline from 'node:readline'
const rl = readline.createInterface({ input: process.stdin })
const out = (obj) => process.stdout.write(JSON.stringify(obj) + '\n')
rl.on('line', (line) => {
  try {
    const m = JSON.parse(line)
    if (m.method === 'initialize') {
      out({ jsonrpc: '2.0', id: m.id, result: { protocolVersion: '2024-11-05', capabilities: {}, serverInfo: { name: 'orprobe', version: '0.0.1' } } })
    } else if (m.method === 'tools/list') {
      out({ jsonrpc: '2.0', id: m.id, result: { tools: [] } })
    } else if (m.id != null && m.method) {
      out({ jsonrpc: '2.0', id: m.id, result: {} })
    }
  } catch {
    /* not json */
  }
})
