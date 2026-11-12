// Agents working in the phone's tabs. The daemon in this app sends
// openremote-browser's calls (src-tauri/src/mobile_browser.rs hands each one
// here) and gets the answers the desktop gives: a snapshot is {text, url,
// title}, a picture {data, mime, url, title}, an action {url, title, text?,
// note?}. Pages are read through the shared agent helper (agent.js), which
// every page gets at its start; taps and keys arrive as real touch and key
// events, so the page can't tell them from a person's. Private tabs never
// reach here unless the person hands one over.

package space.moreweb.openremote.browser

import android.graphics.Bitmap
import android.graphics.Canvas
import android.os.SystemClock
import android.util.Base64
import android.view.InputDevice
import android.view.MotionEvent
import android.view.View
import android.webkit.JsPromptResult
import android.webkit.WebView
import app.tauri.plugin.JSObject
import org.json.JSONArray
import org.json.JSONException
import org.json.JSONObject
import java.util.UUID
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

/** A refusal or failure, said to the agent as it is. */
class Fail(message: String) : Exception(message)

/** An answer given once, however many paths reach it. */
class Once(private val reply: (Any?, String?) -> Unit) {
  private var done = false

  fun ok(value: Any?) {
    if (done) return
    done = true
    reply(value, null)
  }

  fun fail(message: String?) {
    if (done) return
    done = true
    reply(null, message ?: "something went wrong in the page")
  }
}

/** A field's text, or null when it is missing or JSON's null (never the text "null"). */
fun JSONObject.text(key: String): String? = if (isNull(key)) null else optString(key)

/** A string as JavaScript source. */
internal fun quote(s: String?): String = if (s == null) "null" else JSONObject.quote(s)

private const val HAS_HELPERS =
  "typeof globalThis.__openremote === 'object' && typeof globalThis.__openremote_host === 'object'"

/** The widest picture an agent gets, in pixels. */
private const val MAX_SHOT = 1280

/** A whole-page picture stops after this many screens, and this many pixels tall. */
private const val MAX_SCREENS = 8.0
private const val MAX_TALL = 8000.0

/** How much of a page's file comes over per call. */
private const val CHUNK = 512 * 1024

private const val NO_UPLOAD =
  "Files can't be put into a page on the phone from a chat yet: the chat's files are on the computer that runs it."

private class BlobRead(val bytes: ByteArray, val type: String?)

class AgentHost(private val browser: Browser) {
  private val ui get() = browser.ui

  /** One call; [reply] hears its answer (a JSON value) or a problem in words. */
  fun run(call: JSONObject, reply: (Any?, String?) -> Unit) {
    val out = Once(reply)
    guard(out) { dispatch(call, out) }
  }

  private inline fun guard(out: Once, body: () -> Unit) {
    try {
      body()
    } catch (e: Exception) {
      out.fail(e.message ?: e.toString())
    }
  }

  private fun after(ms: Long, out: Once, body: () -> Unit) {
    ui.postDelayed({ guard(out) { body() } }, ms)
  }

  private fun dispatch(call: JSONObject, out: Once) {
    when (val op = call.optString("op")) {
      "tabs" -> out.ok(JSONArray().apply { for (tab in browser.tabs.values) if (tab.forAgents) put(browser.tabInfo(tab)) })
      "open" -> open(call, out)
      "mark" -> {
        call.text("tab")?.let { browser.tabs[it] }?.takeIf { it.forAgents }?.let {
          browser.mark(it, call.optJSONObject("controller"))
        }
        out.ok(JSObject())
      }
      else -> act(op, target(call), call, out)
    }
  }

  private fun act(op: String, tab: Tab, call: JSONObject, out: Once) {
    when (op) {
      "close" -> {
        val said = result(tab, "Closed.")
        browser.close(tab)
        out.ok(said)
      }
      "select" -> {
        browser.emit("select", JSObject().apply { put("id", tab.id) })
        out.ok(result(tab, "The browser shows this tab now."))
      }
      "navigate" -> navigate(tab, call.text("url") ?: throw Fail("'url' is needed"), out)
      "back" -> history(tab, -1, out)
      "forward" -> history(tab, 1, out)
      "reload" -> reload(tab, out)
      "snapshot" -> snapshot(tab, out)
      "click" -> click(tab, call, out)
      "hover" -> hover(tab, call, out)
      "type" -> type(tab, call, out)
      "press" -> press(tab, call.text("key") ?: throw Fail("'key' is needed"), out)
      "select_option" -> selectOption(tab, call, out)
      "scroll" -> scroll(tab, call, out)
      "screenshot" -> screenshot(tab, call, out)
      "console" -> lines(tab, tab.consoleLines.drain(), out)
      "network" -> lines(tab, tab.networkLines.drain(), out)
      "evaluate" -> evaluate(tab, call.text("script") ?: throw Fail("'script' is needed"), out)
      "wait" -> wait(tab, call, out)
      "viewport" -> viewport(tab, call, out)
      "dialog" -> dialog(tab, call, out)
      "upload" -> throw Fail(NO_UPLOAD)
      else -> throw Fail("the phone's browser can't do '$op' yet")
    }
  }

  private fun target(call: JSONObject): Tab {
    val id = call.text("tab") ?: throw Fail("name a tab (browser_tabs lists them)")
    return browser.tabs[id]?.takeIf { it.forAgents } ?: throw Fail("there's no tab $id on the phone")
  }

  /** The tab's page, awake and rendering behind the console while the agent works. */
  private fun live(tab: Tab): WebView {
    if (tab.crashed) throw Fail("The page in this tab stopped working. Reload it (browser_go with reload), then try again.")
    browser.busy(tab)
    return tab.page ?: throw Fail("the tab's page couldn't be opened")
  }

  private fun ref(call: JSONObject): String =
    call.text("target")?.removePrefix("ref=")?.takeIf { it.isNotBlank() }
      ?: throw Fail("name the element's ref from browser_snapshot")

  private fun result(tab: Tab, text: String? = null, note: String? = null): JSObject = JSObject().apply {
    put("url", tab.page?.url ?: tab.url)
    put("title", tab.page?.title ?: tab.title)
    if (text != null) put("text", text)
    val notes = listOfNotNull(note, tab.dialog?.let { "The page is showing a ${it.kind}: \"${it.message.take(300)}\". Answer it with browser_dialog." })
    if (notes.isNotEmpty()) put("note", notes.joinToString("\n"))
  }

  // ---- running script in the page ----

  /** Makes sure the page has the helpers (a page that began before they existed, about:blank). */
  private fun ready(page: WebView, out: Once, then: () -> Unit) {
    page.evaluateJavascript(HAS_HELPERS) { has ->
      guard(out) {
        if (has == "true") then() else page.evaluateJavascript(browser.scripts) { guard(out) { then() } }
      }
    }
  }

  /** Evaluates [expr] in the page; [then] gets {ok: its value}. A throw in the page fails the call. */
  private fun js(tab: Tab, page: WebView, expr: String, out: Once, then: (JSONObject) -> Unit) {
    ready(page, out) {
      val wrapped = "(() => { try { return { ok: ($expr) } } catch (e) { return { error: String((e && e.message) || e) } } })()"
      page.evaluateJavascript(wrapped) { raw ->
        guard(out) {
          if (tab.page !== page) throw Fail("the tab closed")
          val answer = parse(raw)
          answer.text("error")?.let { throw Fail(it) }
          then(answer)
        }
      }
    }
  }

  private fun parse(raw: String?): JSONObject {
    if (raw == null || raw == "null") return JSONObject().put("error", "the page didn't answer (it may still be loading)")
    return try {
      JSONObject(raw)
    } catch (_: JSONException) {
      JSONObject().put("error", "the page answered oddly")
    }
  }

  /** A string the page answered with, or null. */
  private fun unquote(raw: String?): String? {
    if (raw == null || raw == "null") return null
    return try {
      JSONArray("[$raw]").getString(0)
    } catch (_: JSONException) {
      null
    }
  }

  /** Runs an async function's [body] in the page and waits for what it returns. */
  private fun later(tab: Tab, page: WebView, body: String, timeoutMs: Long, out: Once, then: (JSONObject) -> Unit) {
    val slot = UUID.randomUUID().toString()
    ready(page, out) {
      page.evaluateJavascript("__openremote_host.run('$slot', async () => {\n$body\n})", null)
      await(tab, page, slot, SystemClock.uptimeMillis() + timeoutMs, out, then)
    }
  }

  /** evaluateJavascript can't wait for a promise: the answer is fetched until it is there. */
  private fun await(tab: Tab, page: WebView, slot: String, until: Long, out: Once, then: (JSONObject) -> Unit) {
    page.evaluateJavascript("__openremote_host.take('$slot')") { raw ->
      guard(out) {
        if (tab.page !== page) throw Fail("the tab closed while the page was working")
        val answer = if (raw == null || raw == "null") null else parse(raw)
        when {
          answer != null && !answer.optBoolean("pending") -> {
            answer.text("error")?.let { throw Fail(it) }
            then(answer)
          }
          SystemClock.uptimeMillis() > until -> throw Fail("the page took too long to answer")
          else -> after(80, out) { await(tab, page, slot, until, out, then) }
        }
      }
    }
  }

  // ---- going places ----

  private fun open(call: JSONObject, out: Once) {
    val url = call.text("url")?.trim()?.takeIf { it.isNotEmpty() } ?: throw Fail("'url' is needed")
    browser.refused(url)?.let { throw Fail(it) }
    val tab = browser.open(url, null, false, false, call.optJSONObject("controller"))
    browser.busy(tab)
    // The agent reads the page next: answer once its first load is done.
    settle(tab, 0, 20_000, out) { out.ok(browser.tabInfo(tab)) }
  }

  private fun navigate(tab: Tab, url: String, out: Once) {
    browser.refused(url)?.let { throw Fail(it) }
    val page = live(tab)
    val before = tab.loads
    val from = page.url ?: tab.url
    browser.navigate(tab, url)
    // Only the part after # changes: no load to wait for.
    if (from.substringBefore('#') == url.substringBefore('#') && url.contains('#')) {
      return after(300, out) { out.ok(result(tab)) }
    }
    settle(tab, before, 20_000, out) { done ->
      out.ok(result(tab, note = if (done) null else "It was still loading after 20 seconds."))
    }
  }

  private fun history(tab: Tab, step: Int, out: Once) {
    val page = live(tab)
    val can = if (step < 0) page.canGoBack() else page.canGoForward()
    if (!can) throw Fail(if (step < 0) "there's no page before this one in the tab" else "there's no page after this one in the tab")
    val before = tab.loads
    if (step < 0) page.goBack() else page.goForward()
    settle(tab, before, 15_000, out) { out.ok(result(tab)) }
  }

  private fun reload(tab: Tab, out: Once) {
    val before = tab.loads
    browser.reload(tab)
    browser.busy(tab)
    settle(tab, before, 20_000, out) { done ->
      out.ok(result(tab, note = if (done) null else "It was still loading after 20 seconds."))
    }
  }

  /** Waits for the main-frame load that starts after [before] to finish; true unless it timed out. */
  private fun settle(tab: Tab, before: Int, timeoutMs: Long, out: Once, then: (Boolean) -> Unit) {
    val start = SystemClock.uptimeMillis()
    fun check() {
      val waited = SystemClock.uptimeMillis() - start
      val began = tab.loads > before
      when {
        began && !tab.loading -> then(true)
        // Nothing started (a tap that did nothing, a page that handles it itself).
        !began && waited > 1200 -> then(true)
        waited > timeoutMs -> then(false)
        else -> after(100, out) { check() }
      }
    }
    after(150, out) { check() }
  }

  // ---- reading ----

  private fun snapshot(tab: Tab, out: Once) {
    val page = live(tab)
    js(tab, page, "__openremote.snapshot()", out) { answer -> out.ok(result(tab, answer.optString("ok"))) }
  }

  private fun lines(tab: Tab, lines: List<String>, out: Once) {
    out.ok(result(tab).apply { put("lines", JSONArray(lines)) })
  }

  private fun wait(tab: Tab, call: JSONObject, out: Once) {
    val text = call.text("text")
    val gone = call.text("gone")
    val ms = if (call.isNull("ms")) null else call.optLong("ms").coerceIn(0, 30_000)
    if (text == null && gone == null) {
      val pause = ms ?: 1000L
      return after(pause, out) { out.ok(result(tab, "Waited $pause ms.")) }
    }
    val page = live(tab)
    val until = SystemClock.uptimeMillis() + (ms ?: 30_000L)
    val probe = "(() => { const t = document.body ? document.body.innerText : ''; " +
      "return { here: ${quote(text)} === null || t.includes(${quote(text)}), gone: ${quote(gone)} === null || !t.includes(${quote(gone)}) } })()"
    fun check() {
      page.evaluateJavascript(probe) { raw ->
        guard(out) {
          if (tab.page !== page) throw Fail("the tab closed while waiting")
          val seen = parse(raw)
          if (seen.optBoolean("here") && seen.optBoolean("gone")) {
            out.ok(result(tab, if (text != null) "\"$text\" is on the page." else "\"$gone\" is no longer on the page."))
          } else if (SystemClock.uptimeMillis() > until) {
            throw Fail(if (text != null) "\"$text\" didn't appear in time" else "\"$gone\" was still on the page")
          } else {
            after(250, out) { check() }
          }
        }
      }
    }
    check()
  }

  // ---- acting ----

  /** Where an element is in the page's own pixels, scrolled into view and outlined for the person watching. */
  private fun point(tab: Tab, page: WebView, ref: String, out: Once, then: (Float, Float, String?) -> Unit) {
    val r = quote(ref)
    val expr = "(() => { const p = __openremote.point($r); __openremote.outline($r); const v = globalThis.visualViewport; " +
      "return { x: p.x, y: p.y, covered: p.covered, ox: v ? v.offsetLeft : 0, oy: v ? v.offsetTop : 0, s: v ? v.scale : 1, d: globalThis.devicePixelRatio || 1 } })()"
    js(tab, page, expr, out) { answer ->
      val p = answer.optJSONObject("ok") ?: throw Fail("the page didn't say where $ref is")
      // CSS pixels to the view's: past the pinch zoom's offset, times its scale and the screen's density.
      val k = p.optDouble("s", 1.0) * p.optDouble("d", browser.density.toDouble())
      val x = ((p.optDouble("x") - p.optDouble("ox", 0.0)) * k).toFloat()
      val y = ((p.optDouble("y") - p.optDouble("oy", 0.0)) * k).toFloat()
      if (x.isNaN() || y.isNaN() || x < 0 || y < 0 || x > page.width || y > page.height) {
        throw Fail("$ref isn't where it can be pressed on screen; scroll to it and take a new snapshot")
      }
      then(x, y, p.text("covered"))
    }
  }

  private fun click(tab: Tab, call: JSONObject, out: Once) {
    val ref = ref(call)
    val page = live(tab)
    val button = call.text("button") ?: "left"
    if (button != "left") return otherButton(tab, page, ref, button, out)
    point(tab, page, ref, out) { x, y, covered ->
      val before = tab.loads
      tap(page, x, y, if (call.optBoolean("double")) 2 else 1, out) {
        settle(tab, before, 10_000, out) {
          out.ok(result(tab, note = covered?.let { "$it was on top of it, so the tap landed there." }))
        }
      }
    }
  }

  /** A touch screen has no right or middle button: the page gets that click as a script event. */
  private fun otherButton(tab: Tab, page: WebView, ref: String, button: String, out: Once) {
    val r = quote(ref)
    val type = if (button == "right") "contextmenu" else "auxclick"
    val code = if (button == "right") 2 else 1
    val expr = "(() => { const el = __openremote.element($r); el.scrollIntoView({ block: 'center', inline: 'center', behavior: 'instant' }); " +
      "const b = el.getBoundingClientRect(); __openremote.outline($r); " +
      "el.dispatchEvent(new MouseEvent('$type', { bubbles: true, cancelable: true, composed: true, button: $code, clientX: b.left + b.width / 2, clientY: b.top + b.height / 2 })); return true })()"
    js(tab, page, expr, out) {
      out.ok(result(tab, note = "The phone has no $button button, so the page got a $button click from script."))
    }
  }

  private fun tap(page: View, x: Float, y: Float, times: Int, out: Once, then: () -> Unit) {
    val down = SystemClock.uptimeMillis()
    touch(page, down, down, MotionEvent.ACTION_DOWN, x, y)
    after(60, out) {
      touch(page, down, SystemClock.uptimeMillis(), MotionEvent.ACTION_UP, x, y)
      if (times > 1) after(90, out) { tap(page, x, y, times - 1, out, then) } else after(250, out) { then() }
    }
  }

  private fun touch(page: View, down: Long, at: Long, action: Int, x: Float, y: Float) {
    val event = pointer(down, at, action, x, y, MotionEvent.TOOL_TYPE_FINGER, InputDevice.SOURCE_TOUCHSCREEN)
    page.dispatchTouchEvent(event)
    event.recycle()
  }

  private fun pointer(down: Long, at: Long, action: Int, px: Float, py: Float, tool: Int, source: Int): MotionEvent {
    val properties = arrayOf(MotionEvent.PointerProperties().apply {
      id = 0
      toolType = tool
    })
    val coords = arrayOf(MotionEvent.PointerCoords().apply {
      x = px
      y = py
      pressure = if (tool == MotionEvent.TOOL_TYPE_MOUSE) 0f else 1f
      size = 1f
    })
    return MotionEvent.obtain(down, at, action, 1, properties, coords, 0, 0, 1f, 1f, 0, 0, source, 0)
  }

  private fun hover(tab: Tab, call: JSONObject, out: Once) {
    val ref = ref(call)
    val page = live(tab)
    point(tab, page, ref, out) { x, y, _ ->
      // A mouse's hover, which a finger can't give: the page gets mouseover and mousemove.
      val now = SystemClock.uptimeMillis()
      for (action in intArrayOf(MotionEvent.ACTION_HOVER_ENTER, MotionEvent.ACTION_HOVER_MOVE)) {
        val event = pointer(now, now, action, x, y, MotionEvent.TOOL_TYPE_MOUSE, InputDevice.SOURCE_MOUSE)
        page.dispatchGenericMotionEvent(event)
        event.recycle()
      }
      after(250, out) { out.ok(result(tab)) }
    }
  }

  private fun type(tab: Tab, call: JSONObject, out: Once) {
    val ref = ref(call)
    val text = call.text("text") ?: throw Fail("'text' is needed")
    val clear = if (call.isNull("clear")) true else call.optBoolean("clear", true)
    val page = live(tab)
    // Typed where the field's caret is, as an input method would: frameworks
    // see the input events they expect. A field that refuses that is set.
    val expr = """
      ((ref, text, clear) => {
        const o = __openremote
        const field = o.focusField(ref, clear)
        if (!field.editable) throw new Error(ref + ' is not a field you can type into')
        const el = o.element(ref)
        if (!clear && typeof el.setSelectionRange === 'function' && typeof el.value === 'string') {
          try { el.setSelectionRange(el.value.length, el.value.length) } catch (e) {}
        }
        let typed = false
        try { typed = el.ownerDocument.execCommand('insertText', false, text) } catch (e) {}
        if (!typed) o.setValue(ref, text, clear)
        o.outline(ref)
        return typed
      })(${quote(ref)}, ${quote(text)}, $clear)
    """.trimIndent()
    js(tab, page, expr, out) {
      if (call.optBoolean("submit")) press(tab, "Enter", out) else after(150, out) { out.ok(result(tab)) }
    }
  }

  private fun press(tab: Tab, key: String, out: Once) {
    val page = live(tab)
    val before = tab.loads
    val press = Keys.parse(key)
    if (press == null) {
      // A character with no key of its own (é, an emoji): typed where the focus is.
      if (key.codePointCount(0, key.length) != 1) {
        throw Fail("'$key' isn't a key the phone knows; try Enter, Tab, Escape, ArrowDown or Control+A")
      }
      val expr = "(() => { const a = document.activeElement; const d = a ? a.ownerDocument : document; return d.execCommand('insertText', false, ${quote(key)}) })()"
      return js(tab, page, expr, out) { after(100, out) { out.ok(result(tab)) } }
    }
    // The page on screen takes the keys as the person's would; one out of
    // sight gets them without taking the phone's focus from the console.
    if (browser.shown === tab && !page.hasFocus()) page.requestFocus()
    Keys.send(page, press)
    settle(tab, before, 10_000, out) { out.ok(result(tab)) }
  }

  private fun selectOption(tab: Tab, call: JSONObject, out: Once) {
    val ref = ref(call)
    val values = call.optJSONArray("values") ?: JSONArray()
    val page = live(tab)
    val r = quote(ref)
    js(tab, page, "(() => { const chosen = __openremote.selectOptions($r, $values); __openremote.outline($r); return chosen })()", out) { answer ->
      val chosen = answer.optJSONArray("ok") ?: JSONArray()
      val names = (0 until chosen.length()).joinToString(", ") { chosen.optString(it) }
      after(100, out) { out.ok(result(tab, "Chose $names.")) }
    }
  }

  private fun scroll(tab: Tab, call: JSONObject, out: Once) {
    val ref = call.text("target")?.removePrefix("ref=")
    val dx = call.optDouble("dx", 0.0).takeIf { !it.isNaN() } ?: 0.0
    val dy = call.optDouble("dy", 0.0).takeIf { !it.isNaN() } ?: 0.0
    val page = live(tab)
    js(tab, page, "__openremote.scrollBy(${quote(ref)}, $dx, $dy)", out) { answer ->
      val at = answer.optJSONObject("ok")
      val where = if (at != null) " The page is scrolled to ${at.optInt("x")}, ${at.optInt("y")}." else ""
      after(120, out) { out.ok(result(tab, "Scrolled.$where")) }
    }
  }

  private fun evaluate(tab: Tab, script: String, out: Once) {
    val page = live(tab)
    val slot = UUID.randomUUID().toString()
    val finish: (JSONObject) -> Unit = { answer ->
      out.ok(result(tab).apply { if (!answer.optBoolean("none")) put("result", answer.opt("ok") ?: JSONObject.NULL) })
    }
    // What the first look found: still running, done already, or nothing
    // (the source didn't parse in that form).
    val settled: (String?) -> Boolean = { raw ->
      val answer = if (raw == null || raw == "null") null else parse(raw)
      when {
        answer == null -> false
        answer.optBoolean("pending") -> {
          await(tab, page, slot, SystemClock.uptimeMillis() + 30_000, out, finish)
          true
        }
        else -> {
          answer.text("error")?.let { throw Fail(it) }
          finish(answer)
          true
        }
      }
    }
    ready(page, out) {
      // As an expression first, then as a function's body. The source runs
      // as written, never through eval, so a page's script policy can't stop it.
      page.evaluateJavascript("__openremote_host.run('$slot', async () => (\n$script\n))", null)
      page.evaluateJavascript("__openremote_host.take('$slot')") { first ->
        guard(out) {
          if (!settled(first)) {
            page.evaluateJavascript("__openremote_host.run('$slot', async () => {\n$script\n})", null)
            page.evaluateJavascript("__openremote_host.take('$slot')") { second ->
              guard(out) {
                if (!settled(second)) throw Fail("The script didn't run: it isn't valid JavaScript (browser_console has the page's error).")
              }
            }
          }
        }
      }
    }
  }

  private fun viewport(tab: Tab, call: JSONObject, out: Once) {
    if (!call.isNull("size")) {
      throw Fail("The phone shows pages at its own size and can't show this one at another. A computer's browser can.")
    }
    out.ok(result(tab, "The page is at the phone's own size."))
  }

  private fun dialog(tab: Tab, call: JSONObject, out: Once) {
    val open = tab.dialog ?: throw Fail("The page isn't showing a dialog.")
    tab.dialog = null
    val answer = open.result
    if (!call.optBoolean("accept")) {
      answer.cancel()
    } else if (answer is JsPromptResult) {
      answer.confirm(call.text("text") ?: open.value ?: "")
    } else {
      answer.confirm()
    }
    after(200, out) { out.ok(result(tab, "Answered the page's ${open.kind}.")) }
  }

  // ---- pictures ----

  private fun screenshot(tab: Tab, call: JSONObject, out: Once) {
    val page = live(tab)
    val ref = call.text("target")?.removePrefix("ref=")
    when {
      ref != null -> element(tab, page, ref, out)
      call.optBoolean("full_page") -> fullPage(tab, page, out)
      else -> browser.shots.bitmap(tab, MAX_SHOT, true) { shot ->
        guard(out) { picture(tab, shot ?: throw Fail("the page couldn't be pictured"), null, out) }
      }
    }
  }

  private fun picture(tab: Tab, shot: Bitmap, note: String?, out: Once) {
    browser.shots.jpeg(shot, 75) { data ->
      if (data == null) {
        out.fail("the picture couldn't be made")
      } else {
        out.ok(result(tab, note = note).apply {
          put("data", data)
          put("mime", "image/jpeg")
        })
      }
    }
  }

  private fun element(tab: Tab, page: WebView, ref: String, out: Once) {
    val r = quote(ref)
    val expr = "(() => { const el = __openremote.element($r); el.scrollIntoView({ block: 'center', inline: 'center', behavior: 'instant' }); " +
      "const b = el.getBoundingClientRect(); const v = globalThis.visualViewport; " +
      "return { x: b.left, y: b.top, w: b.width, h: b.height, ox: v ? v.offsetLeft : 0, oy: v ? v.offsetTop : 0, s: v ? v.scale : 1, d: globalThis.devicePixelRatio || 1 } })()"
    js(tab, page, expr, out) { answer ->
      val box = answer.optJSONObject("ok") ?: throw Fail("the page didn't say where $ref is")
      val k = box.optDouble("s", 1.0) * box.optDouble("d", browser.density.toDouble())
      val clip = Clip(
        (box.optDouble("x") - box.optDouble("ox", 0.0)) * k,
        (box.optDouble("y") - box.optDouble("oy", 0.0)) * k,
        box.optDouble("w") * k,
        box.optDouble("h") * k,
      )
      // A moment for the scroll to draw.
      after(150, out) {
        browser.shots.bitmap(tab, page.width, true) { shot ->
          guard(out) {
            if (shot == null) throw Fail("the page couldn't be pictured")
            val part = browser.shots.crop(shot, page.width, clip, 1f) ?: throw Fail("$ref couldn't be pictured")
            if (part !== shot) shot.recycle()
            picture(tab, browser.shots.fit(part, MAX_SHOT), null, out)
          }
        }
      }
    }
  }

  /** The whole page, screen by screen down to a limit, as one tall picture. */
  private fun fullPage(tab: Tab, page: WebView, out: Once) {
    val measure = "(() => { const d = document.scrollingElement || document.documentElement; " +
      "return { h: Math.max(d.scrollHeight, innerHeight), vh: innerHeight, x: scrollX, y: scrollY } })()"
    js(tab, page, measure, out) { answer ->
      val m = answer.optJSONObject("ok") ?: throw Fail("the page didn't say how tall it is")
      val screen = m.optDouble("vh").takeIf { it > 0 } ?: throw Fail("the page has no height yet")
      val total = min(m.optDouble("h"), screen * MAX_SCREENS)
      val cut = m.optDouble("h") > total + 1
      val perCss = page.height / screen
      val scale = min(1.0, min(MAX_SHOT.toDouble() / page.width, MAX_TALL / (total * perCss)))
      val width = max(1, (page.width * scale).roundToInt())
      val sheet = try {
        Bitmap.createBitmap(width, max(1, (total * perCss * scale).roundToInt()), Bitmap.Config.ARGB_8888)
      } catch (_: OutOfMemoryError) {
        throw Fail("the page is too large to picture whole; picture a part of it")
      }
      val canvas = Canvas(sheet)
      val startX = m.optDouble("x")
      val startY = m.optDouble("y")
      fun slice(y: Double) {
        if (y >= total) {
          page.evaluateJavascript("scrollTo({ left: $startX, top: $startY, behavior: 'instant' })", null)
          return picture(tab, sheet, if (cut) "The page goes on below: this is its first ${MAX_SCREENS.toInt()} screens." else null, out)
        }
        page.evaluateJavascript("scrollTo({ left: $startX, top: $y, behavior: 'instant' })", null)
        after(180, out) {
          page.evaluateJavascript("scrollY") { now ->
            guard(out) {
              val at = now?.toDoubleOrNull() ?: y
              val shot = browser.shots.draw(page, width) ?: throw Fail("the page couldn't be pictured")
              canvas.drawBitmap(shot, 0f, (at * perCss * scale).toFloat(), null)
              shot.recycle()
              // The last screen, or a page that wouldn't scroll as far as it said.
              if (at + screen >= total || at < y - 1) slice(total) else slice(y + screen)
            }
          }
        }
      }
      slice(0.0)
    }
  }

  // ---- a page's own files ----

  /**
   * A blob: address only the page that made it can read: the page reads it
   * and hands it over in pieces. [done] gets the bytes and their type, or
   * a problem.
   */
  fun readBlob(tab: Tab, url: String, max: Long, done: (ByteArray?, String?, String?) -> Unit) {
    val page = tab.page ?: return done(null, null, "the tab has no page")
    val out = Once { value, error ->
      val read = value as? BlobRead
      if (error != null || read == null) done(null, null, error ?: "the page's file couldn't be read") else done(read.bytes, read.type, null)
    }
    val key = UUID.randomUUID().toString()
    val body = """
      const response = await fetch(${quote(url)})
      const blob = await response.blob()
      if (blob.size > $max) throw new Error('that file is too large to save from the page')
      const data = await new Promise((resolve, reject) => {
        const reader = new FileReader()
        reader.onload = () => resolve(String(reader.result))
        reader.onerror = () => reject(reader.error)
        reader.readAsDataURL(blob)
      })
      const at = data.indexOf(',')
      __openremote_host.keep(${quote(key)}, data.slice(at + 1))
      return { type: blob.type, length: data.length - at - 1 }
    """.trimIndent()
    later(tab, page, body, 120_000, out) { answer ->
      val meta = answer.optJSONObject("ok") ?: throw Fail("the page didn't read its file")
      val length = meta.optLong("length")
      val text = StringBuilder()
      fun piece(from: Long) {
        if (from >= length) {
          page.evaluateJavascript("__openremote_host.drop(${quote(key)})", null)
          val bytes = try {
            Base64.decode(text.toString(), Base64.DEFAULT)
          } catch (_: IllegalArgumentException) {
            throw Fail("the page's file came over garbled")
          }
          return out.ok(BlobRead(bytes, meta.text("type")))
        }
        page.evaluateJavascript("__openremote_host.slice(${quote(key)}, $from, $CHUNK)") { raw ->
          guard(out) {
            if (tab.page !== page) throw Fail("the tab closed while saving its file")
            text.append(unquote(raw) ?: throw Fail("the page lost its file"))
            piece(from + CHUNK)
          }
        }
      }
      piece(0)
    }
  }
}
