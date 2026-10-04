// Cloud mode's coming-soon state: the main area while the mode switch
// sits on Cloud. The subject itself is the hero — the cloud with
// machines checking in around it, in the app's own status palette —
// drawn in once, then the words under it: status pill, title, subtext,
// one action.

import { House } from '@phosphor-icons/react'
import './cloudmode.css'

// One machine checking in: a small card with its status dot — green for
// online, hollow for not-yet. The same vocabulary the machine cards use.
function MachineNode({ x, y, online, delay }) {
  return (
    <g className="cm-node" style={{ '--cm-delay': `${delay}ms` }}>
      <rect x={x} y={y} width={26} height={19} rx={5} />
      <circle className={online ? 'cm-ndot on' : 'cm-ndot'} cx={x + 8} cy={y + 9.5} r={2.4} />
      <line x1={x + 14} y1={y + 9.5} x2={x + 21} y2={y + 9.5} />
    </g>
  )
}

export default function CloudMode({ onBackToLocal }) {
  return (
    <div className="cloudmode">
      <div className="cm-panel">
        <svg
          className="cm-hero"
          viewBox="0 0 260 170"
          role="img"
          aria-label="A cloud with machines checking in around it"
        >
          {/* the connectors: each machine reaching the cloud */}
          <line className="cm-link" style={{ '--cm-delay': '300ms' }} x1="34" y1="46" x2="86" y2="72" />
          <line className="cm-link" style={{ '--cm-delay': '420ms' }} x1="224" y1="38" x2="176" y2="66" />
          <line className="cm-link" style={{ '--cm-delay': '540ms' }} x1="18" y1="132" x2="82" y2="102" />
          <line className="cm-link" style={{ '--cm-delay': '660ms' }} x1="238" y1="126" x2="180" y2="100" />

          {/* the cloud itself — lucide's own cloud path, drawn in */}
          <path
            className="cm-cloud"
            d="M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9Z"
            transform="translate(68 34) scale(5.2)"
          />

          <MachineNode x={8} y={36} online delay={300} />
          <MachineNode x={212} y={28} online delay={420} />
          <MachineNode x={4} y={124} delay={540} />
          <MachineNode x={226} y={118} delay={660} />
        </svg>

        <p className="cm-status">
          <span className="cm-dot" aria-hidden="true" />
          Coming soon
        </p>

        <h1 className="cm-title">Every machine, one console</h1>

        <p className="cm-body">
          Cloud mode connects the harnesses on machines you reach over the network — cloud hosts,
          home servers, anything that checks in. Until it lands, everything stays on this computer.
        </p>

        <button type="button" className="cm-back" onClick={onBackToLocal}>
          <House size={13} />
          Back to Local
        </button>
      </div>
    </div>
  )
}
