// The built-in browser as a Tauri plugin. Only the app's Rust side calls
// these commands (src-tauri/src/mobile_browser.rs, which gives the console
// the desktop's browser_* commands and runs the daemon's agent calls); no
// page and no permission in capabilities/ reaches them from JavaScript.
// What the pages do goes back on the one channel `start` hands over.

package space.moreweb.openremote.browser

import android.app.Activity
import android.webkit.WebView
import androidx.appcompat.app.AppCompatActivity
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import org.json.JSONObject

@InvokeArg
class StartArgs {
  lateinit var events: Channel
  var agentScript: String? = null
}

@TauriPlugin
class BrowserPlugin(activity: Activity) : Plugin(activity) {
  private val browser = Browser(activity as AppCompatActivity)

  // The console's webview is the activity's content now: the layers can go beside it.
  override fun load(webView: WebView) {
    browser.attach()
  }

  override fun onResume(activity: AppCompatActivity) {
    browser.rebind(activity)
    browser.ensureAttached()
    browser.resumed()
  }

  override fun onPause(activity: AppCompatActivity) {
    browser.paused()
  }

  override fun onDestroy(activity: AppCompatActivity) {
    if (activity === browser.activity) browser.destroyed()
  }

  /** Runs a command; what [body] returns resolves it, what it throws rejects it in words. */
  private fun answer(invoke: Invoke, body: (JSObject) -> JSObject?) {
    try {
      invoke.resolve(body(invoke.getArgs()))
    } catch (e: Exception) {
      invoke.reject(e.message ?: e.toString())
    }
  }

  private fun tabOf(args: JSObject): Tab {
    val id = args.text("tab") ?: throw Fail("name a tab")
    return browser.tabs[id] ?: throw Fail("no tab $id")
  }

  /** The app's Rust side, once: the events channel and the agent helper; answers what this browser is. */
  @Command
  fun start(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(StartArgs::class.java)
      val events = args.events
      browser.sink = { event, payload ->
        events.send(JSObject().apply {
          put("event", event)
          put("payload", payload)
        })
      }
      args.agentScript?.takeIf { it.isNotBlank() }?.let { browser.scripts = HOST_SCRIPT + ";\n" + it }
      browser.forgetLeftovers()
      invoke.resolve(browser.hello())
    } catch (e: Exception) {
      invoke.reject(e.message ?: e.toString())
    }
  }

  @Command
  fun open(invoke: Invoke) = answer(invoke) { args ->
    val url = args.text("url")?.trim().orEmpty()
    browser.refused(url)?.let { throw Fail(it) }
    val isPrivate = args.optBoolean("private")
    // Without WebView profiles a private tab would share the normal tabs'
    // cookies and storage, and what it set would outlive it: it is refused
    // rather than opened as private in name only.
    if (isPrivate && !browser.profiles) {
      throw Fail("this phone's Android System WebView is too old to keep a private tab apart; update it from the Play Store, then try again")
    }
    browser.ensureAttached()
    val tab = browser.open(
      url,
      browser.frameOf(args.optJSONObject("bounds")),
      args.optBoolean("show"),
      isPrivate,
      null,
    )
    JSObject().apply { put("id", tab.id) }
  }

  @Command
  fun tabs(invoke: Invoke) = answer(invoke) { JSObject().apply { put("tabs", browser.tabList()) } }

  @Command
  fun close(invoke: Invoke) = answer(invoke) { args ->
    browser.close(tabOf(args))
    null
  }

  @Command
  fun setBounds(invoke: Invoke) = answer(invoke) { args ->
    browser.setFrame(tabOf(args), browser.frameOf(args.optJSONObject("bounds")))
    null
  }

  @Command
  fun show(invoke: Invoke) = answer(invoke) { args ->
    browser.ensureAttached()
    browser.show(tabOf(args), args.optBoolean("focus"))
    null
  }

  @Command
  fun hide(invoke: Invoke) = answer(invoke) { args ->
    browser.hide(tabOf(args))
    null
  }

  @Command
  fun navigate(invoke: Invoke) = answer(invoke) { args ->
    val url = args.text("url")?.trim().orEmpty()
    browser.refused(url)?.let { throw Fail(it) }
    browser.navigate(tabOf(args), url)
    null
  }

  @Command
  fun back(invoke: Invoke) = answer(invoke) { args ->
    tabOf(args).page?.let { if (it.canGoBack()) it.goBack() }
    null
  }

  @Command
  fun forward(invoke: Invoke) = answer(invoke) { args ->
    tabOf(args).page?.let { if (it.canGoForward()) it.goForward() }
    null
  }

  @Command
  fun reload(invoke: Invoke) = answer(invoke) { args ->
    browser.reload(tabOf(args))
    null
  }

  @Command
  fun stop(invoke: Invoke) = answer(invoke) { args ->
    browser.stop(tabOf(args))
    null
  }

  @Command
  fun capture(invoke: Invoke) {
    try {
      val args = invoke.getArgs()
      val tab = tabOf(args)
      val clip = args.optJSONObject("clip")
        ?.let { Clip(it.optDouble("x", 0.0), it.optDouble("y", 0.0), it.optDouble("width", 0.0), it.optDouble("height", 0.0)) }
        ?.takeIf { it.width > 0 && it.height > 0 }
      val max = args.optDouble("max", Double.NaN).takeIf { !it.isNaN() && it > 0 }
      browser.shots.capture(tab, clip, max) { data ->
        if (data == null) invoke.reject("no picture of that page") else invoke.resolve(JSObject().apply { put("data", data) })
      }
    } catch (e: Exception) {
      invoke.reject(e.message ?: e.toString())
    }
  }

  @Command
  fun zoom(invoke: Invoke) = answer(invoke) { args ->
    browser.zoom(tabOf(args), args.optDouble("factor", 1.0))
    null
  }

  @Command
  fun find(invoke: Invoke) = answer(invoke) { args ->
    browser.find(tabOf(args), args.text("term").orEmpty(), args.optInt("step", 0))
    null
  }

  @Command
  fun permission(invoke: Invoke) {
    try {
      val args = invoke.getArgs()
      val id = args.optLong("id", -1L)
      browser.permissions.answer(id, args.optBoolean("allow")) { problem ->
        if (problem == null) invoke.resolve() else invoke.reject(problem)
      }
    } catch (e: Exception) {
      invoke.reject(e.message ?: e.toString())
    }
  }

  @Command
  fun clearSite(invoke: Invoke) = answer(invoke) { args ->
    browser.clearSite(tabOf(args))?.let { throw Fail(it) }
    null
  }

  @Command
  fun clearData(invoke: Invoke) = answer(invoke) { args ->
    val kinds = args.optJSONArray("kinds")
    browser.clearData((0 until (kinds?.length() ?: 0)).mapNotNull { kinds?.optString(it) }.toSet())
    null
  }

  @Command
  fun openExternal(invoke: Invoke) = answer(invoke) { args ->
    val url = args.text("url").orEmpty()
    if (!url.startsWith("https://") && !url.startsWith("http://")) throw Fail("only web pages open outside")
    browser.files.openOutside(url)?.let { throw Fail(it) }
    null
  }

  @Command
  fun share(invoke: Invoke) = answer(invoke) { args ->
    val tab = args.text("tab")?.let { browser.tabs[it] }
    val url = args.text("url") ?: tab?.let { it.page?.url ?: it.url } ?: throw Fail("there's nothing to share")
    browser.files.share(url, args.text("title") ?: tab?.title)?.let { throw Fail(it) }
    null
  }

  @Command
  fun saveFile(invoke: Invoke) {
    try {
      val args = invoke.getArgs()
      browser.files.saveFile(args.text("name").orEmpty(), args.text("data").orEmpty()) { path, problem ->
        if (path != null) invoke.resolve(JSObject().apply { put("path", path) }) else invoke.reject(problem ?: "that file couldn't be saved")
      }
    } catch (e: Exception) {
      invoke.reject(e.message ?: e.toString())
    }
  }

  @Command
  fun reveal(invoke: Invoke) = answer(invoke) { args ->
    browser.files.reveal(args.text("path").orEmpty())?.let { throw Fail(it) }
    null
  }

  @Command
  fun desktopSite(invoke: Invoke) = answer(invoke) { args ->
    browser.desktop(tabOf(args), args.optBoolean("on"))
    null
  }

  @Command
  fun shareWithAgents(invoke: Invoke) = answer(invoke) { args ->
    browser.shareWithAgents(tabOf(args), args.optBoolean("shared"))
    null
  }

  @Command
  fun download(invoke: Invoke) = answer(invoke) { args ->
    val url = args.text("url").orEmpty()
    val scheme = url.substringBefore(':').lowercase()
    if (scheme !in setOf("http", "https", "data", "blob")) throw Fail("that isn't something to download")
    browser.files.downloadLink(tabOf(args), url)
    null
  }

  /** One of the daemon's calls (openremote-browser's Call); answers {ok: its answer}. */
  @Command
  fun agent(invoke: Invoke) {
    val call = invoke.getArgs().optJSONObject("call") ?: return invoke.reject("no call")
    browser.agent.run(call) { value, problem ->
      if (problem != null) invoke.reject(problem) else invoke.resolve(JSObject().apply { put("ok", value ?: JSONObject.NULL) })
    }
  }
}
