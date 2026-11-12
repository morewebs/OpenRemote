// The address bar's suggestion list: the rows the pure rule ranked, drawn
// under the field. The field keeps the focus (aria-activedescendant points at
// the highlighted row), so typing keeps working while the list is open.

export default function BrowserSuggestions({ id, rows, active, onPick }) {
  if (!rows.length) return null
  return (
    <div className="bw-suggest" id={id} role="listbox" aria-label="Suggestions" onMouseDown={(e) => e.preventDefault()}>
      {rows.map((row, i) => (
        <div
          key={row.newTab ? `new-tab-${row.url}` : row.url}
          id={`${id}-${i}`}
          role="option"
          aria-selected={i === active}
          className={i === active ? 'bw-suggest-row active' : 'bw-suggest-row'}
          onClick={(e) => onPick(row, { newTab: e.ctrlKey || e.metaKey || row.newTab })}
        >
          <span className="bw-suggest-title">{row.title || row.url}</span>
          <span className="bw-suggest-url">{row.url}</span>
          {row.kept && <span className="bw-suggest-kept">Bookmark</span>}
        </div>
      ))}
    </div>
  )
}
