// 36×36px (or custom size) visual identity tile for a plugin. Renders an
// official brand mark when recognized, a category fallback icon if no
// monogram is available, or a clean 2-letter monogram.

import { useState } from 'react'
import {
  AmazonLogo,
  AndroidLogo,
  Code,
  Cpu,
  Database,
  FigmaLogo,
  FilePdf,
  GithubLogo,
  GitlabLogo,
  Lightning,
  Palette,
  ShieldCheck,
  SlackLogo,
  Wrench,
} from '@phosphor-icons/react'
import { siDocker, siJupyter, siPython } from 'simple-icons'
import { pluginInitials, resolvePluginBrand } from './plugins.js'

const BRAND_ICONS = {
  github: ({ size }) => <GithubLogo size={size} />,
  gitlab: ({ size }) => <GitlabLogo size={size} />,
  docker: ({ size }) => (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="currentColor"
      role="img"
      aria-label="Docker"
      className="pg-icon-svg"
    >
      <path d={siDocker.path} />
    </svg>
  ),
  android: ({ size }) => <AndroidLogo size={size} />,
  aws: ({ size }) => <AmazonLogo size={size} />,
  slack: ({ size }) => <SlackLogo size={size} />,
  pdf: ({ size }) => <FilePdf size={size} />,
  jupyter: ({ size }) => (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="currentColor"
      role="img"
      aria-label="Jupyter"
      className="pg-icon-svg"
    >
      <path d={siJupyter.path} />
    </svg>
  ),
  python: ({ size }) => (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="currentColor"
      role="img"
      aria-label="Python"
      className="pg-icon-svg"
    >
      <path d={siPython.path} />
    </svg>
  ),
  figma: ({ size }) => <FigmaLogo size={size} />,
}

const CATEGORY_ICONS = {
  development: Code,
  dev: Code,
  coding: Code,
  security: ShieldCheck,
  productivity: Lightning,
  design: Palette,
  data: Database,
  database: Database,
  ai: Cpu,
  tools: Wrench,
  utilities: Wrench,
}

function iconUrl(homepage) {
  const text = String(homepage ?? '').trim()
  const github = text.match(/github\.com\/([^/?#]+)/i)
  if (github && github[1] && github[1] !== 'settings') {
    return `https://github.com/${github[1]}.png?size=64`
  }
  try {
    const host = new URL(text).hostname
    if (host) return `https://www.google.com/s2/favicons?domain=${encodeURIComponent(host)}&sz=64`
  } catch {
    /* not a page */
  }
  return null
}

export function PluginIcon({
  name = '',
  id = '',
  category = '',
  homepage = '',
  size = 36,
  className = '',
  style = {},
}) {
  const brand = resolvePluginBrand(id, name)
  const iconSize = Math.max(14, Math.round(size * 0.52))
  const remote = iconUrl(homepage)
  const [remoteFailed, setRemoteFailed] = useState(false)

  let content = null

  if (remote && !remoteFailed) {
    content = (
      <img
        className="pg-icon-img"
        src={remote}
        alt=""
        width={size}
        height={size}
        onError={() => setRemoteFailed(true)}
      />
    )
  } else if (brand && BRAND_ICONS[brand]) {
    const BrandComponent = BRAND_ICONS[brand]
    content = <BrandComponent size={iconSize} />
  } else {
    const initials = pluginInitials(name)
    const catKey = (category ?? '').trim().toLowerCase()
    const CategoryIcon = catKey ? CATEGORY_ICONS[catKey] : null

    if (initials && initials !== 'PL') {
      content = (
        <span
          className="pg-icon-monogram"
          style={{ fontSize: `${Math.max(10, Math.round(size * 0.36))}px` }}
        >
          {initials}
        </span>
      )
    } else if (CategoryIcon) {
      content = <CategoryIcon size={iconSize} />
    } else {
      content = (
        <span
          className="pg-icon-monogram"
          style={{ fontSize: `${Math.max(10, Math.round(size * 0.36))}px` }}
        >
          {initials || 'PL'}
        </span>
      )
    }
  }

  return (
    <div
      className={className ? `pg-icon-tile ${className}` : 'pg-icon-tile'}
      style={{ width: size, height: size, minWidth: size, minHeight: size, ...style }}
      aria-hidden="true"
    >
      {content}
    </div>
  )
}

export default PluginIcon
