// An agent's key presses ("Enter", "Control+A", "ArrowDown") as Android key
// events, which the page receives as real keyboard input.

package space.moreweb.openremote.browser

import android.os.SystemClock
import android.view.KeyEvent
import android.view.View

object Keys {
  class Press(val code: Int, val meta: Int)

  private val NAMED = mapOf(
    "enter" to KeyEvent.KEYCODE_ENTER,
    "return" to KeyEvent.KEYCODE_ENTER,
    "tab" to KeyEvent.KEYCODE_TAB,
    "escape" to KeyEvent.KEYCODE_ESCAPE,
    "esc" to KeyEvent.KEYCODE_ESCAPE,
    "backspace" to KeyEvent.KEYCODE_DEL,
    "delete" to KeyEvent.KEYCODE_FORWARD_DEL,
    "del" to KeyEvent.KEYCODE_FORWARD_DEL,
    "insert" to KeyEvent.KEYCODE_INSERT,
    "arrowup" to KeyEvent.KEYCODE_DPAD_UP,
    "arrowdown" to KeyEvent.KEYCODE_DPAD_DOWN,
    "arrowleft" to KeyEvent.KEYCODE_DPAD_LEFT,
    "arrowright" to KeyEvent.KEYCODE_DPAD_RIGHT,
    "up" to KeyEvent.KEYCODE_DPAD_UP,
    "down" to KeyEvent.KEYCODE_DPAD_DOWN,
    "left" to KeyEvent.KEYCODE_DPAD_LEFT,
    "right" to KeyEvent.KEYCODE_DPAD_RIGHT,
    "home" to KeyEvent.KEYCODE_MOVE_HOME,
    "end" to KeyEvent.KEYCODE_MOVE_END,
    "pageup" to KeyEvent.KEYCODE_PAGE_UP,
    "pagedown" to KeyEvent.KEYCODE_PAGE_DOWN,
    "space" to KeyEvent.KEYCODE_SPACE,
    " " to KeyEvent.KEYCODE_SPACE,
    "f1" to KeyEvent.KEYCODE_F1,
    "f2" to KeyEvent.KEYCODE_F2,
    "f3" to KeyEvent.KEYCODE_F3,
    "f4" to KeyEvent.KEYCODE_F4,
    "f5" to KeyEvent.KEYCODE_F5,
    "f6" to KeyEvent.KEYCODE_F6,
    "f7" to KeyEvent.KEYCODE_F7,
    "f8" to KeyEvent.KEYCODE_F8,
    "f9" to KeyEvent.KEYCODE_F9,
    "f10" to KeyEvent.KEYCODE_F10,
    "f11" to KeyEvent.KEYCODE_F11,
    "f12" to KeyEvent.KEYCODE_F12,
  )

  private val SIGNS = mapOf(
    ',' to KeyEvent.KEYCODE_COMMA,
    '.' to KeyEvent.KEYCODE_PERIOD,
    '/' to KeyEvent.KEYCODE_SLASH,
    ';' to KeyEvent.KEYCODE_SEMICOLON,
    '\'' to KeyEvent.KEYCODE_APOSTROPHE,
    '[' to KeyEvent.KEYCODE_LEFT_BRACKET,
    ']' to KeyEvent.KEYCODE_RIGHT_BRACKET,
    '-' to KeyEvent.KEYCODE_MINUS,
    '=' to KeyEvent.KEYCODE_EQUALS,
    '`' to KeyEvent.KEYCODE_GRAVE,
    '\\' to KeyEvent.KEYCODE_BACKSLASH,
    '+' to KeyEvent.KEYCODE_PLUS,
    '*' to KeyEvent.KEYCODE_STAR,
    '@' to KeyEvent.KEYCODE_AT,
    '#' to KeyEvent.KEYCODE_POUND,
  )

  private const val SHIFT = KeyEvent.META_SHIFT_ON or KeyEvent.META_SHIFT_LEFT_ON

  /** A key or chord, or null when the phone has no key for it. */
  fun parse(chord: String): Press? {
    val text = chord.trim()
    if (text.isEmpty()) return null
    // "Control++" ends in the plus key itself.
    val parts = if (text.endsWith("++")) text.dropLast(2).split('+') + "+" else text.split('+')
    var meta = 0
    for (mod in parts.dropLast(1)) {
      meta = meta or when (mod.trim().lowercase()) {
        "control", "ctrl" -> KeyEvent.META_CTRL_ON or KeyEvent.META_CTRL_LEFT_ON
        "shift" -> SHIFT
        "alt", "option" -> KeyEvent.META_ALT_ON or KeyEvent.META_ALT_LEFT_ON
        // A Mac's Command chords mean Control's on Android (select all,
        // copy, paste), which is what an agent asking for them wants.
        "meta", "command", "cmd", "super", "controlormeta" -> KeyEvent.META_CTRL_ON or KeyEvent.META_CTRL_LEFT_ON
        else -> return null
      }
    }
    val key = parts.last().let { if (it.isBlank() && it.isNotEmpty()) it else it.trim() }
    NAMED[key.lowercase()]?.let { return Press(it, meta) }
    if (key.length != 1) return null
    val c = key[0]
    return when {
      c in 'a'..'z' -> Press(KeyEvent.KEYCODE_A + (c - 'a'), meta)
      c in 'A'..'Z' -> Press(KeyEvent.KEYCODE_A + (c - 'A'), meta or SHIFT)
      c in '0'..'9' -> Press(KeyEvent.KEYCODE_0 + (c - '0'), meta)
      else -> SIGNS[c]?.let { Press(it, meta) }
    }
  }

  /** A press and its release, to the page in [view]. */
  fun send(view: View, press: Press) {
    val now = SystemClock.uptimeMillis()
    view.dispatchKeyEvent(KeyEvent(now, now, KeyEvent.ACTION_DOWN, press.code, 0, press.meta))
    view.dispatchKeyEvent(KeyEvent(now, now + 40, KeyEvent.ACTION_UP, press.code, 0, press.meta))
  }
}
