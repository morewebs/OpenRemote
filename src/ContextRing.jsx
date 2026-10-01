import { formatTokens, ringGeometry } from './world.js'
import './contextring.css'

export default function ContextRing({ used = 0, window = 200000 }) {
  const ring = ringGeometry(used, window)
  const pct = ring.pct
  return (
    <div className="nc-ring-wrap">
      <svg className="nc-ring" viewBox="0 0 20 20" width="20" height="20">
        <circle className="nc-ring-track" cx="10" cy="10" r="8" />
        <circle
          className="nc-ring-fill"
          cx="10"
          cy="10"
          r="8"
          strokeDasharray={ring.circumference}
          strokeDashoffset={ring.offset}
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
          <span>{formatTokens(ring.remaining)}</span>
        </div>
        <div className="nc-ctx-row">
          <span>Auto-compact</span>
          <span>92%</span>
        </div>
      </div>
    </div>
  )
}
