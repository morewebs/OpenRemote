// The harnesses' real brand marks, as inline SVG geometry. Each is the
// actual logo its own users know it by, verbatim from its owner's own
// published asset, normalized to a common component (no redraws, no
// reinterpretation). Each keeps its own official color - a mark in a
// stranger's grey stops being that mark. Rendered at identification
// sizes only; every mark stays a trademark of its owner.
//
//   claude     Anthropic's asterisk - simple-icons' official path
//   codex      the OpenAI hexapod (Codex has no mark of its own) -
//              simple-icons v11.6.0's official OpenAI path
//   grok       xAI's mark - wikimedia commons XAI-Logo.svg, the four
//              polygons verbatim, scaled to 24
//   pi         the pi coding agent's block glyph - pi.dev's own
//              logo-auto.svg, its three color paths verbatim
//   opencode   OpenCode's mark - simple-icons' official path
//   agy        Antigravity's wave - the shape path from its own favicon

import { siClaudecode, siOpencode } from 'simple-icons'

// simple-icons dropped the OpenAI mark in v12; this is their last
// published official path (v11.6.0), byte for byte.
const OPENAI_PATH =
  'M22.2819 9.8211a5.9847 5.9847 0 0 0-.5157-4.9108 6.0462 6.0462 0 0 0-6.5098-2.9A6.0651 6.0651 0 0 0 4.9807 4.1818a5.9847 5.9847 0 0 0-3.9977 2.9 6.0462 6.0462 0 0 0 .7427 7.0966 5.98 5.98 0 0 0 .511 4.9107 6.051 6.051 0 0 0 6.5146 2.9001A5.9847 5.9847 0 0 0 13.2599 24a6.0557 6.0557 0 0 0 5.7718-4.2058 5.9894 5.9894 0 0 0 3.9977-2.9001 6.0557 6.0557 0 0 0-.7475-7.0729zm-9.022 12.6081a4.4755 4.4755 0 0 1-2.8764-1.0408l.1419-.0804 4.7783-2.7582a.7948.7948 0 0 0 .3927-.6813v-6.7369l2.02 1.1686a.071.071 0 0 1 .038.052v5.5826a4.504 4.504 0 0 1-4.4945 4.4944zm-9.6607-4.1254a4.4708 4.4708 0 0 1-.5346-3.0137l.142.0852 4.783 2.7582a.7712.7712 0 0 0 .7806 0l5.8428-3.3685v2.3324a.0804.0804 0 0 1-.0332.0615L9.74 19.9502a4.4992 4.4992 0 0 1-6.1408-1.6464zM2.3408 7.8956a4.485 4.485 0 0 1 2.3655-1.9728V11.6a.7664.7664 0 0 0 .3879.6765l5.8144 3.3543-2.0201 1.1685a.0757.0757 0 0 1-.071 0l-4.8303-2.7865A4.504 4.504 0 0 1 2.3408 7.872zm16.5963 3.8558L13.1038 8.364 15.1192 7.2a.0757.0757 0 0 1 .071 0l4.8303 2.7913a4.4944 4.4944 0 0 1-.6765 8.1042v-5.6772a.79.79 0 0 0-.407-.667zm2.0107-3.0231l-.142-.0852-4.7735-2.7818a.7759.7759 0 0 0-.7854 0L9.409 9.2297V6.8974a.0662.0662 0 0 1 .0284-.0615l4.8303-2.7866a4.4992 4.4992 0 0 1 6.6802 4.66zM8.3065 12.863l-2.02-1.1638a.0804.0804 0 0 1-.038-.0567V6.0742a4.4992 4.4992 0 0 1 7.3757-3.4537l-.142.0805L8.704 5.459a.7948.7948 0 0 0-.3927.6813zm1.0976-2.3654l2.602-1.4998 2.6069 1.4998v2.9994l-2.5974 1.4997-2.6067-1.4997Z'

// One mark: full-bleed svg that fills the box the parent sizes. The
// fill comes from the mark's own CSS color variable (each mark sets
// --mark), not currentColor - a brand color is part of the mark.
function Mark({ viewBox, color, children, title }) {
  return (
    <svg
      className="brand-mark"
      viewBox={viewBox}
      style={{ '--mark': color }}
      role="img"
      aria-label={title}
    >
      {children}
    </svg>
  )
}

export function ClaudeMark() {
  // #D97757 - Anthropic's own terracotta (simple-icons' official hex).
  return (
    <Mark viewBox="0 0 24 24" color="#D97757" title="Claude Code">
      <path fill="var(--mark)" d={siClaudecode.path} />
    </Mark>
  )
}

export function CodexMark() {
  // The hexapod is monochrome in OpenAI's own usage; on this dark UI,
  // white is the official dark-mode rendition (openai.com dark header).
  return (
    <Mark viewBox="0 0 24 24" color="#FFFFFF" title="Codex">
      <path fill="var(--mark)" d={OPENAI_PATH} />
    </Mark>
  )
}

export function GrokMark() {
  // xAI renders its mark black-on-light or white-on-dark (x.ai itself
  // is dark with a white mark). White it is.
  return (
    <Mark viewBox="0 0 466.04 516.93" color="#FFFFFF" title="Grok Build">
      <g fill="var(--mark)">
        <polygon points="0.12 182.71 234.14 516.92 338.15 516.92 104.13 182.71 0.12 182.71" />
        <polygon points="0 516.92 104.08 516.92 156.08 442.67 104.04 368.34 0 516.92" />
        <polygon points="466.04 0 361.96 0 182.1 256.86 234.15 331.18 466.04 0" />
        <polygon points="380.78 516.92 466.04 516.92 466.04 37.16 380.78 158.92 380.78 516.92" />
      </g>
    </Mark>
  )
}

export function PiMark() {
  // pi.dev's own block glyph - the three colors are the mark, so this
  // one keeps them.
  return (
    <svg className="brand-mark" viewBox="0 0 800 800" role="img" aria-label="Pi Agent">
      <path fill="#F09082" d="M165.29 165.29H517.36V400H400V282.65H165.29Z" />
      <path fill="#4D9ABF" d="M165.29 282.65H282.65V400H400V517.36H282.65V634.72H165.29Z" />
      <path fill="#F1BE58" d="M517.36 400H634.72V634.72H517.36Z" />
    </svg>
  )
}

export function OpencodeMark() {
  // OpenCode's own mark is black; white is its dark-mode rendition.
  return (
    <Mark viewBox="0 0 24 24" color="#FFFFFF" title="OpenCode">
      <path fill="var(--mark)" d={siOpencode.path} />
    </Mark>
  )
}

export function AntigravityMark() {
  // The wave in Antigravity's own blue. The viewBox is cropped to the
  // path's own bounds - its favicon's 136x136 box exists for the
  // background disc, and the wave occupies only the middle of it, so
  // the full box left the mark visibly smaller than its neighbors.
  return (
    <Mark viewBox="10 17 88 80" color="#3186FF" title="Antigravity">
      <path
        fill="var(--mark)"
        d="M89.6992 93.695C94.3659 97.195 101.366 94.8617 94.9492 88.445C75.6992 69.7783 79.7825 18.445 55.8659 18.445C31.9492 18.445 36.0325 69.7783 16.7825 88.445C9.78251 95.445 17.3658 97.195 22.0325 93.695C40.1159 81.445 38.9492 59.8617 55.8659 59.8617C72.7825 59.8617 71.6159 81.445 89.6992 93.695Z"
      />
    </Mark>
  )
}

// The registry: harness id -> its real mark. This is the one map the
// whole console uses; there are no fallback glyphs anymore.
const MARKS = {
  claude: ClaudeMark,
  codex: CodexMark,
  grok: GrokMark,
  pi: PiMark,
  opencode: OpencodeMark,
  agy: AntigravityMark,
}

// Renders a harness's brand mark at the given size, or null for an
// unknown id.
export function HarnessMark({ harness, size = 14 }) {
  const M = MARKS[harness]
  if (!M) return null
  return (
    <span className="brand-mark-box" style={{ width: size, height: size }}>
      <M />
    </span>
  )
}
