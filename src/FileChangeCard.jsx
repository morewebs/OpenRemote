import { useId, useState } from 'react'
import { CaretDown, CaretRight } from '@phosphor-icons/react'
import './file-change-card.css'

export default function FileChangeCard({ file, result }) {
  const [open, setOpen] = useState(false)
  const id = useId()
  const shown = file.lines.slice(0, 2000)
  return (
    <div className={`file-change${file.failed ? ' is-failed' : ''}${file.pending ? ' is-pending' : ''}`}>
      <button type="button" className="file-change-toggle" onClick={() => setOpen(value => !value)} aria-expanded={open} aria-controls={id}>
        <span className="file-change-action">{file.label}</span>
        <span className="file-change-name pv" title={file.path}>{file.name}</span>
        {file.added > 0 && <span className="file-change-added" aria-label={`${file.added} lines added`}>+{file.added}</span>}
        {file.removed > 0 && <span className="file-change-removed" aria-label={`${file.removed} lines removed`}>−{file.removed}</span>}
        {open ? <CaretDown size={12} /> : <CaretRight size={12} />}
      </button>
      {open && (
        <section className="file-change-panel" id={id} aria-label={`Changes to ${file.name}`}>
          <div className="file-change-path pv">{file.path}{file.excerpt && <span>Changed excerpt</span>}</div>
          {file.failed && <pre className="file-change-error pv">{typeof result === 'string' ? result : 'The App reported that this change failed.'}</pre>}
          <div className="file-change-code pv" tabIndex={0} role="region" aria-label={`File content for ${file.name}`}>
            {shown.length === 0 ? <div className="file-change-empty">Empty file</div> : shown.map((line, i) => (
              <div className={`file-change-line is-${line.kind}`} key={i}>
                <span className="file-change-number" aria-hidden="true">{line.number}</span>
                <span className="file-change-sign" aria-hidden="true">{line.kind === 'add' ? '+' : line.kind === 'del' ? '−' : ' '}</span>
                <code>{line.text || ' '}</code>
              </div>
            ))}
          </div>
          {file.lines.length > shown.length && <p className="file-change-limit">Showing the first {shown.length.toLocaleString()} lines of {file.lines.length.toLocaleString()}.</p>}
        </section>
      )}
    </div>
  )
}
