// The icons a plugin may name for its sidebar tile and composer button -
// the console's own set, so a plugin's place in the app looks like the
// app. plugin-runtime.js holds the names (iconName folds `Microphone`,
// `microphone` and `magic-wand` into one key); an unknown one never reaches
// here (it becomes the plugin piece there).

import {
  BookOpen,
  Calendar,
  Camera,
  ChartBar,
  ChatCircle,
  Clock,
  Code,
  Globe,
  Image as ImageGlyph,
  Lightning,
  ListChecks,
  MagicWand,
  Microphone,
  NotePencil,
  Notebook,
  Paperclip,
  PuzzlePiece,
  Robot,
  Sparkle,
  Terminal,
  Translate,
  Waveform,
} from '@phosphor-icons/react'

const GLYPHS = {
  microphone: Microphone,
  waveform: Waveform,
  translate: Translate,
  sparkle: Sparkle,
  magicwand: MagicWand,
  notepencil: NotePencil,
  notebook: Notebook,
  bookopen: BookOpen,
  listchecks: ListChecks,
  chatcircle: ChatCircle,
  camera: Camera,
  image: ImageGlyph,
  paperclip: Paperclip,
  globe: Globe,
  code: Code,
  terminal: Terminal,
  robot: Robot,
  lightning: Lightning,
  clock: Clock,
  calendar: Calendar,
  chartbar: ChartBar,
  puzzlepiece: PuzzlePiece,
}

export function PluginGlyph({ name, size = 16, ...rest }) {
  const Glyph = GLYPHS[name] ?? PuzzlePiece
  return <Glyph size={size} {...rest} />
}
