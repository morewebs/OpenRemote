import { defaultAgentAvatar } from './agent-avatar.js'
import { HarnessMark } from './brand-marks.jsx'
import { harnessLabel } from './harness-names.js'
import { chatTitle } from './shape.js'
import './agent-avatar.css'

export default function AgentAvatar({ agent, size = 30, showAppBadge = false }) {
  const { initials, color } = defaultAgentAvatar(agent.id, chatTitle(agent))
  const app = showAppBadge && agent.harness ? harnessLabel(agent.harness) : null
  return (
    <span className="agent-avatar" data-color={color} style={{ '--avatar-base': `${size}px` }}>
      <span className="agent-avatar-initials pv" aria-hidden="true">{initials}</span>
      {app && (
        <span className="agent-avatar-app" role="img" aria-label={`App: ${app}`} title={app}>
          <HarnessMark harness={agent.harness} size={size >= 34 ? 12 : size >= 26 ? 11 : 9} />
        </span>
      )}
    </span>
  )
}
