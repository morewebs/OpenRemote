// The model makers' real marks, the same contract as the harnesses'
// (brand-marks.jsx): each is the actual logo its own users know it by,
// verbatim from its owner's own published asset (simple-icons' official
// paths, byte for byte; OpenAI's and xAI's ride over from brand-marks
// where they already live). Each keeps its official brand color - a
// mark in a stranger's grey stops being that mark; where the official
// color is near-black, white is the company's own dark-mode rendition.
// Rendered at identification sizes only; every mark stays a trademark
// of its owner.
//
//   claude     Anthropic's Claude starburst - the Claude app's own
//              logo (claude.svg), not the CLI harness's blocky glyph
//   openai     the hexapod - shared with the codex harness mark
//   xai        xAI's four-polygon mark - shared with the grok harness
//   gemini     Google's Gemini star
//   deepseek   DeepSeek's whale
//   qwen       Qwen's mark
//   mistral    Mistral's grid
//   meta       Meta's infinity loop
//   kimi       Moonshot's Kimi mark
//
// An unknown maker renders nothing - text alone, never a wrong mark.

import {
  siClaude,
  siDeepseek,
  siGooglegemini,
  siKimi,
  siMeta,
  siMistralai,
  siQwen,
} from 'simple-icons'
import { XAI_POLYGONS } from './brand-marks.jsx'
import chatgptUrl from './assets/chatgpt.png'

// One mark: the same anatomy as brand-marks.jsx - full-bleed svg that
// fills the box the parent sizes, fill from the mark's own color
// variable, never currentColor.
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

export function ClaudeModelMark() {
  // #D97757 - Anthropic's official Claude terracotta.
  return (
    <Mark viewBox="0 0 24 24" color="#D97757" title="Claude">
      <path fill="var(--mark)" d={siClaude.path} />
    </Mark>
  )
}

export function OpenAIModelMark() {
  return <img className="brand-mark" src={chatgptUrl} alt="ChatGPT" />
}

export function XaiModelMark() {
  // xAI's mark, white on dark (x.ai itself is dark with a white mark).
  return (
    <Mark viewBox="0 0 466.04 516.93" color="#FFFFFF" title="xAI">
      <g fill="var(--mark)">
        {XAI_POLYGONS.map((points) => (
          <polygon key={points} points={points} />
        ))}
      </g>
    </Mark>
  )
}

export function GeminiModelMark() {
  // #8E75B2 - Google's official Gemini purple.
  return (
    <Mark viewBox="0 0 24 24" color="#8E75B2" title="Gemini">
      <path fill="var(--mark)" d={siGooglegemini.path} />
    </Mark>
  )
}

export function DeepseekModelMark() {
  // #5786FE - DeepSeek's official blue.
  return (
    <Mark viewBox="0 0 24 24" color="#5786FE" title="DeepSeek">
      <path fill="var(--mark)" d={siDeepseek.path} />
    </Mark>
  )
}

export function QwenModelMark() {
  // #6950EF - Qwen's official violet.
  return (
    <Mark viewBox="0 0 24 24" color="#6950EF" title="Qwen">
      <path fill="var(--mark)" d={siQwen.path} />
    </Mark>
  )
}

export function MistralModelMark() {
  // #FA520F - Mistral's official orange.
  return (
    <Mark viewBox="0 0 24 24" color="#FA520F" title="Mistral">
      <path fill="var(--mark)" d={siMistralai.path} />
    </Mark>
  )
}

export function MetaModelMark() {
  // #0467DF - Meta's official blue.
  return (
    <Mark viewBox="0 0 24 24" color="#0467DF" title="Meta">
      <path fill="var(--mark)" d={siMeta.path} />
    </Mark>
  )
}

export function KimiModelMark() {
  // Kimi's own mark is black; white is its dark-mode rendition.
  return (
    <Mark viewBox="0 0 24 24" color="#FFFFFF" title="Kimi">
      <path fill="var(--mark)" d={siKimi.path} />
    </Mark>
  )
}

// The registry: model-maker.js's maker key -> its mark, plus an optical
// size factor (the same ink-balancing discipline as the harness set -
// the boxes are equal, the ink is not). Optical scaling only, never a
// redraw.
const MAKERS = {
  claude: { M: ClaudeModelMark, optical: 1.05 },
  openai: { M: OpenAIModelMark, optical: 0.94 },
  xai: { M: XaiModelMark, optical: 1 },
  gemini: { M: GeminiModelMark, optical: 1.05 },
  deepseek: { M: DeepseekModelMark, optical: 0.95 },
  qwen: { M: QwenModelMark, optical: 0.95 },
  mistral: { M: MistralModelMark, optical: 1 },
  meta: { M: MetaModelMark, optical: 1 },
  kimi: { M: KimiModelMark, optical: 1.05 },
}

// Renders a model maker's mark at the given size, or null for an
// unknown maker (model-maker.js answers null itself for a catalog it
// can't name).
export function ModelMark({ maker, size = 14 }) {
  const mark = MAKERS[maker]
  if (!mark) return null
  return (
    <span
      className="brand-mark-box"
      style={{ width: size, height: size, '--mark-scale': mark.optical }}
    >
      <mark.M />
    </span>
  )
}
