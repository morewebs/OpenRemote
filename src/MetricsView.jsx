// Metrics: the session record, counted - statuses, the synced split,
// the harnesses, the rhythm of new chats - beside this machine's own
// presence band. Everything derives from the poll. The running totals a
// chat folds (cost, thinking tokens) exist only for chats this console
// has watched, so they stay out until the daemon can aggregate them
// itself.

import { useConsole } from './state/console.jsx'
import { harnessName } from './harness-names.js'
import {
  createdPerDay,
  harnessCounts,
  machinePresence,
  newChatsLine,
  presenceLine,
  statusSentence,
  syncSplit,
} from './metrics.js'
import { activeSessions } from './archive.js'
import './devices.css'
import './panels.css'
import './metrics.css'

// The one date format both captions read in, so a bar's tooltip and the line
// under the strip never disagree about how a day is written.
const dayLabel = (ms) =>
  new Date(ms).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })

// One quiet bar: the count as height against the window's busiest day,
// never lower than the zero stub.
function DayBar({ band, max }) {
  const day = dayLabel(band.start)
  return (
    <span
      className={`mx-bar${band.count === 0 ? ' mx-bar--zero' : ''}`}
      style={band.count === 0 ? undefined : { height: `${Math.max(4, Math.round((band.count / max) * 28))}px` }}
      title={`${band.count === 1 ? '1 chat' : `${band.count} chats`} · ${day}`}
    />
  )
}

export default function MetricsView() {
  const { sessions, sessionsAnswered, machines, devices, thisDevice, archived } = useConsole()
  // The same list every other surface counts: archived chats are not on the record
  // a person is looking at, so a Metrics total that included them disagreed with the
  // sidebar under it (22 on the record while the lists showed 19).
  const list = activeSessions(sessions, archived ?? new Set())
  const sentence = statusSentence(list)
  const split = syncSplit(list, thisDevice)
  const harnesses = harnessCounts(list)
  const days = createdPerDay(list, 14)
  const dayMax = Math.max(...days.map((d) => d.count), 1)
  const anyDays = days.some((d) => d.count > 0)
  const presence = machinePresence(machines)
  const onlineCount = (devices ?? []).filter((d) => d.online).length

  return (
    <div className="panel">
      <header className="dv-head">
        <div>
          <h1 className="dv-title">Metrics</h1>
          <p className="dv-meta">
            {sessionsAnswered
              ? `${list.length} ${list.length === 1 ? 'chat' : 'chats'} on record`
              : 'Reading the record…'}
          </p>
        </div>
      </header>

      {/* "Nothing on record yet" is a claim about the daemon's answer, so it is
          only made once the record has answered: a cold start would otherwise
          say the user has never run a chat. */}
      {!sessionsAnswered ? (
        <p className="pn-empty">Reading the record…</p>
      ) : list.length === 0 ? (
        <p className="pn-empty">No chats on record yet. Start one - it shows up here.</p>
      ) : (
        <>
          <h2 className="pn-section">Chats on record</h2>
          <div className="pn-list pn-list--tight">
            <div className="pn-row">
              <div>
                <div className="pn-name">
                  {list.length} {list.length === 1 ? 'chat' : 'chats'}, every status
                </div>
                {/* The sentence names only the nonzero statuses. */}
                <div className="pn-detail">{sentence || 'All idle.'}</div>
              </div>
            </div>
          </div>

          {split.here + split.elsewhere > 0 && (
            <>
              <h2 className="pn-section">In your Cloud</h2>
              <div className="pn-list pn-list--tight">
                <div className="pn-row">
                  <div>
                    <div className="pn-name">
                      {split.here + split.elsewhere} synced{' '}
                      {split.here + split.elsewhere === 1 ? 'chat' : 'chats'}
                    </div>
                    <div className="pn-detail">
                      {split.here === 1 ? '1 runs' : `${split.here} run`} here ·{' '}
                      {split.elsewhere === 1 ? '1 runs' : `${split.elsewhere} run`} on other devices
                      {(devices ?? []).length > 0
                        ? ` · ${onlineCount} device${onlineCount === 1 ? '' : 's'} online`
                        : ''}
                    </div>
                  </div>
                </div>
              </div>
            </>
          )}

          {harnesses.length > 0 && (
            <>
              <h2 className="pn-section">By app</h2>
              <div className="pn-list pn-list--tight">
                {harnesses.map((h) => (
                  <div className="pn-row" key={h.harness}>
                    <div>
                      <div className="pn-name">{harnessName(h.harness)}</div>
                      <div className="pn-detail">
                        {h.count} {h.count === 1 ? 'chat' : 'chats'}
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            </>
          )}

          {anyDays && (
            <>
              <h2 className="pn-section">New chats</h2>
              <div className="mx-strip">
                {days.map((band) => (
                  <DayBar key={band.key} band={band} max={dayMax} />
                ))}
              </div>
              {/* The bars carry no numbers, so the line under them says what
                  the window is and which day in it was the busiest. */}
              <p className="mx-cap">{newChatsLine(days, dayLabel)}</p>
            </>
          )}
        </>
      )}

      {presence && (
        <>
          <h2 className="pn-section">Agent presence</h2>
          <div className="mx-band">
            {presence.map((on, i) => (
              <span key={i} className={`mx-cell${on ? ' mx-cell--on' : ''}`} />
            ))}
            {/* The band ends at now; the hairline is where that is, and the
                caption's right-hand word names it. */}
            <span className="mx-now" aria-hidden="true" />
          </div>
          {/* One line for the number the band only shows as colour: the hours
              it was online, and what one cell is worth. */}
          <p className="mx-cap mx-cap--band">
            <span>{presenceLine(presence)}</span>
            <span className="mx-now-label">now</span>
          </p>
        </>
      )}
    </div>
  )
}
