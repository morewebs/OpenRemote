// The context-window donut: the harness's own numbers, never ours. The
// ring renders only where the harness reports its window (codex does;
// claude's stream-json carries usage but no window — its ring stays
// hidden until a wire fact exists).

import './contextring.css'

function formatTokens(n) {
  if (n >= 1_000_000) {
    const m = n / 1_000_000
    return Number.isInteger(m) ? `${m}M` : `${m.toFixed(1)}M`
  }
  if (n >= 1000) return `${Math.round(n / 1000)}k`
  return String(Math.max(0, Math.round(n)))
}

export default function ContextRing({ used = 0, window = 0 }) {
  if (!window) return null
  const circumference = 2 * Math.PI * 8
  const frac = Math.min(1, Math.max(0, used / window))
  const offset = circumference * (1 - frac)
  const pct = Math.round(frac * 100)
  const remaining = Math.max(0, window - used)
  return (
    <div className="nc-ring-wrap">
      <svg className="nc-ring" viewBox="0 0 20 20" width="20" height="20">
        <circle className="nc-ring-track" cx="10" cy="10" r="8" />
        <circle
          className="nc-ring-fill"
          cx="10"
          cy="10"
          r="8"
          strokeDasharray={circumference}
          strokeDashoffset={offset}
        />
      </svg>
      <div className="nc-ctx-card">
        <div className="nc-ctx-title">Context</div>
        <div className="nc-ctx-num">
          {formatTokens(used)} <span>/ {formatTokens(window)} tokens</span>
        </div>
        <div className="nc-ctx-bar">
          <span style={{ width: `${pct}%` }} />
        </div>
        <div className="nc-ctx-row">
          <span>Used</span>
          <span>{pct}%</span>
        </div>
        <div className="nc-ctx-row">
          <span>Remaining</span>
          <span>{formatTokens(remaining)}</span>
        </div>
      </div>
    </div>
  )
}
