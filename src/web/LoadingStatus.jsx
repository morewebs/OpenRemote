import './loading.css'

export default function LoadingStatus({ children = 'Loading...', full = false }) {
  return (
    <div className={`web-loading${full ? ' web-loading--full' : ''}`} role="status">
      <span className="web-loading-spinner" aria-hidden="true" />
      <span>{children}</span>
    </div>
  )
}
