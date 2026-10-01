export default function BrandIcon({ icon, size = 13 }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <path d={icon.path} />
    </svg>
  )
}

export function HarnessIcon({ harness, size = 14 }) {
  if (!harness) return null
  if (harness.brand) return <BrandIcon icon={harness.brand} size={size - 1} />
  const Icon = harness.icon
  return <Icon size={size} weight="light" />
}
