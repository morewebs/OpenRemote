// The built-in browser on the phone. Tauri can't add a second webview to a
// window on Android, so each tab is a native android.webkit.WebView of its
// own, laid over the console exactly where the console draws the page area
// (src/BrowserPhone.jsx reports that rectangle). The console draws all of
// the browser's chrome; this owns the pages and says what they do through
// events (`browser:*` in the console, via src-tauri/src/mobile_browser.rs).
//
// Two layers share the console's place in the activity's content view
// (MainActivity pads it clear of the system bars, the cutout and the
// keyboard): the stage above the console holds the one page on screen, and
// the wings below it hold the other live pages, so a page an agent works in
// keeps rendering while the console covers it. At most five pages stay
// live; the rest are kept as their address and history and come back when
// shown. Tabs use a WebView profile of their own where the device's WebView
// has profiles (pages never share storage, cookies or a renderer with the
// console), and private tabs another, wiped when the last one closes.

package space.moreweb.openremote.browser

import android.animation.Animator
import android.animation.AnimatorListenerAdapter
import android.animation.ValueAnimator
import android.annotation.SuppressLint
import android.content.ComponentCallbacks2
import android.content.Context
import android.content.res.Configuration
import android.graphics.Bitmap
import android.graphics.Color
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.Message
import android.os.SystemClock
import android.util.Base64
import android.view.Choreographer
import android.view.Gravity
import android.view.HapticFeedbackConstants
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import android.view.animation.DecelerateInterpolator
import android.webkit.CookieManager
import android.webkit.GeolocationPermissions
import android.webkit.JsResult
import android.webkit.WebChromeClient
import android.webkit.WebSettings
import android.webkit.WebStorage
import android.webkit.WebView
import android.widget.FrameLayout
import androidx.activity.OnBackPressedCallback
import androidx.appcompat.app.AppCompatActivity
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.webkit.ProfileStore
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import app.tauri.plugin.JSObject
import org.json.JSONArray
import org.json.JSONObject
import java.io.ByteArrayOutputStream
import kotlin.math.min
import kotlin.math.roundToInt

/** Live pages kept at once; the least recently used beyond this are put to sleep. */
private const val MAX_LIVE = 5

/** How long after an agent's last action its page keeps rendering out of sight. */
private const val AGENT_LINGER = 90_000L

private const val PROFILE = "openremote-browser"
private const val PRIVATE_PROFILE = "openremote-private"

/** The console's own addresses, which a tab never opens and never clears. */
private val CONSOLE_HOSTS = setOf("tauri.localhost", "127.0.0.1", "localhost")

class Browser(activity: AppCompatActivity) {
  var activity: AppCompatActivity = activity
    private set
  val ui = Handler(Looper.getMainLooper())
  val density: Float get() = activity.resources.displayMetrics.density

  /** Where events go: the plugin's channel to the app's Rust side. */
  var sink: ((String, JSONObject) -> Unit)? = null

  /** What every page runs at its start, so agents can read and act on it. */
  var scripts = HOST_SCRIPT

  /** Every tab, oldest first. */
  val tabs = LinkedHashMap<String, Tab>()

  /** The tab whose page is on screen. */
  var shown: Tab? = null
    private set

  val permissions = Permissions(this)
  val files = Files(this)
  val shots = Shots(this)
  val agent = AgentHost(this)

  private var counter = 0
  private var stage = layer()
  private var wings = layer().apply {
    // Pages out of sight are no part of what the screen reader walks.
    importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
  }
  private var attached = false
  private var iconsOn = false
  private var fullscreen: Fullscreen? = null

  // The console's bar slides away while the page scrolls down: `offset` is
  // how far, in pixels; `grown` is whether the page was made taller so no
  // gap opens at the bottom while it slides.
  private var offset = 0
  private var grown = false
  private var touching = false
  private var snap: ValueAnimator? = null
  private var chromeQueued = false
  private val settle = Runnable { settleBar() }
  private val sweep = Runnable { sweepAgents() }

  /** Whether this device's WebView keeps profiles apart (WebView 121 and later). */
  val profiles: Boolean by lazy { WebViewFeature.isFeatureSupported(WebViewFeature.MULTI_PROFILE) }

  private val defaultAgent: String by lazy { WebSettings.getDefaultUserAgent(activity) }

  // Back goes to the page's own history first; with none left the console
  // takes back (the opener tab, then the chat).
  private val backPress = object : OnBackPressedCallback(false) {
    override fun handleOnBackPressed() {
      val full = fullscreen
      val page = shown?.page
      when {
        full != null -> full.callback.onCustomViewHidden()
        page != null && page.canGoBack() -> page.goBack()
        else -> isEnabled = false
      }
    }
  }

  private val memory = object : ComponentCallbacks2 {
    // Only real pressure puts pages to sleep: leaving the screen for a moment
    // (TRIM_MEMORY_UI_HIDDEN, all newer Android says) keeps them.
    @Suppress("DEPRECATION")
    override fun onTrimMemory(level: Int) {
      if (level == ComponentCallbacks2.TRIM_MEMORY_RUNNING_CRITICAL || level >= ComponentCallbacks2.TRIM_MEMORY_COMPLETE) {
        ui.post { sleepAllBut(1) }
      }
    }

    override fun onConfigurationChanged(newConfig: Configuration) {}

    @Deprecated("Deprecated in Java")
    override fun onLowMemory() {
      ui.post { sleepAllBut(1) }
    }
  }

  private class Fullscreen(val tab: Tab, val view: View, val callback: WebChromeClient.CustomViewCallback)

  private fun layer(): FrameLayout = FrameLayout(activity).apply {
    clipChildren = true
    isClickable = false
    isFocusable = false
  }

  // ---- the activity ----

  /**
   * Puts the layers beside the console. Only once the console's webview is
   * the content view: setting the content view clears the views beside it.
   */
  fun attach() {
    val content = activity.findViewById<ViewGroup>(android.R.id.content) ?: return
    if (wings.parent == null) {
      content.addView(wings, 0, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
    }
    if (stage.parent == null) {
      content.addView(stage, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
      // The keyboard, a rotation: the layers change size before the console
      // reports its new layout, and the page follows at once.
      stage.addOnLayoutChangeListener { _, left, top, right, bottom, oldLeft, oldTop, oldRight, oldBottom ->
        if (right - left != oldRight - oldLeft || bottom - top != oldBottom - oldTop) {
          ui.post { shown?.let { place(it) } }
        }
      }
    }
    if (!attached) {
      attached = true
      activity.registerComponentCallbacks(memory)
    }
    for (tab in tabs.values) {
      val view = tab.view ?: continue
      if (view.parent == null) {
        if (tab === shown) onstage(tab) else offstage(tab)
      }
    }
    claimBack()
  }

  /**
   * The app came back with a new activity (the old one was finished while
   * the process lived on): the pages belonged to the old one, so they sleep
   * and wake into the new one.
   */
  fun rebind(next: AppCompatActivity) {
    if (next === activity) return
    exitFullscreen()
    for (tab in tabs.values) sleep(tab)
    (stage.parent as? ViewGroup)?.removeView(stage)
    (wings.parent as? ViewGroup)?.removeView(wings)
    if (attached) activity.unregisterComponentCallbacks(memory)
    attached = false
    activity = next
    stage = layer()
    wings = layer().apply { importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS }
    backPress.remove()
    attach()
  }

  /** Leaving the screen: pages nobody works in rest; an agent's keeps going. */
  fun paused() {
    for (tab in tabs.values) if (!agentBusy(tab)) tab.page?.onPause()
  }

  fun resumed() {
    for (tab in tabs.values) tab.page?.onResume()
  }

  /** The activity is going: its pages go with it, their tabs keep their history. */
  fun destroyed() {
    exitFullscreen()
    for (tab in tabs.values) sleep(tab)
  }

  /**
   * The layers are beside the console again if something cleared the
   * content view (a new activity's webview arriving after them).
   */
  fun ensureAttached() {
    if (stage.parent == null || wings.parent == null) attach()
  }

  /** Ours must be the newest back callback, so it is asked before the console's. */
  private fun claimBack() {
    backPress.remove()
    activity.onBackPressedDispatcher.addCallback(activity, backPress)
    updateBack()
  }

  private fun updateBack() {
    val page = shown?.page
    val frame = shown?.frame
    backPress.isEnabled = fullscreen != null ||
      (page != null && page.canGoBack() && frame?.back != false && frame?.parked != true)
  }

  // ---- events ----

  fun emit(event: String, payload: JSONObject) {
    sink?.invoke(event, payload)
  }

  fun changed(tab: Tab) {
    if (tabs[tab.id] === tab) emit("tab", tabJson(tab))
  }

  /** A tab as the console hears it (the desktop's fields and the phone's own). */
  fun tabJson(tab: Tab): JSObject = JSObject().apply {
    put("id", tab.id)
    put("url", tab.url)
    put("title", tab.title)
    put("loading", tab.loading)
    put("progress", tab.progress)
    put("favicon", tab.favicon ?: JSONObject.NULL)
    put("can_back", tab.canBack)
    put("can_forward", tab.canForward)
    put("opener", tab.opener ?: JSONObject.NULL)
    put("controller", tab.controller ?: JSONObject.NULL)
    put("private", tab.isPrivate)
    put("shared", tab.shared)
    put("fullscreen", tab.fullscreen)
    put("desktop", tab.desktop)
    put("crashed", tab.crashed)
    put("live", tab.view != null)
    put("zoom", tab.textZoom / 100.0)
    put("problem", tab.problem ?: JSONObject.NULL)
    // Whether a private tab keeps nothing: false where the WebView has no
    // profiles, so the console can say so.
    put("isolated", !tab.isPrivate || profiles)
    put("agents", tab.forAgents)
  }

  /** A tab as the daemon's agents see it (openremote-browser's TabInfo). */
  fun tabInfo(tab: Tab): JSObject = JSObject().apply {
    put("id", tab.id)
    put("url", tab.page?.url ?: tab.url)
    put("title", tab.page?.title ?: tab.title)
    put("loading", tab.loading)
    tab.opener?.let { put("opener", it) }
    tab.controller?.let { put("controller", it) }
  }

  fun hello(): JSObject = JSObject().apply {
    val version = WebViewCompat.getCurrentWebViewPackage(activity)?.versionName.orEmpty()
    put("engine", "Android WebView $version".trim())
    put("caps", JSObject().apply {
      put("trusted_input", true)
      put("console", true)
      put("network", true)
      put("upload", false)
    })
    put("isolated", profiles)
  }

  // ---- tabs ----

  fun newTab(isPrivate: Boolean, opener: String?): Tab {
    counter += 1
    val tab = Tab("tab-$counter", isPrivate, opener)
    tabs[tab.id] = tab
    return tab
  }

  /** Why a tab may not open this address, or null when it may. */
  fun refused(url: String): String? {
    val uri = try {
      Uri.parse(url.trim())
    } catch (_: Exception) {
      return "that isn't an address"
    }
    return when (uri.scheme?.lowercase()) {
      "http", "https" -> {
        val host = uri.host?.lowercase()?.trimEnd('.') ?: return "that address has no site in it"
        // The app's own names stay out of reach, as on the desktop; plain
        // localhost stays open for the user's own dev servers.
        if (host.endsWith(".localhost")) "$url can't be opened here" else null
      }
      "about", "data" -> null
      else -> "only web pages open in the browser"
    }
  }

  fun open(url: String, frame: Frame?, visible: Boolean, isPrivate: Boolean, controller: JSONObject?): Tab {
    val tab = newTab(isPrivate, null)
    tab.url = url
    tab.loading = true
    tab.frame = frame
    tab.controller = controller
    if (controller != null) tab.agentAt = SystemClock.uptimeMillis()
    val view = makeView(tab)
    view.page.loadUrl(url)
    if (visible) show(tab, false)
    changed(tab)
    return tab
  }

  /** A page opened a window with the user's tap: a tab beside its opener. */
  fun popup(opener: Tab, gesture: Boolean, message: Message): Boolean {
    if (!gesture) return false
    val tab = newTab(opener.isPrivate, opener.id)
    tab.loading = true
    tab.frame = opener.frame
    if (agentBusy(opener)) tab.agentAt = SystemClock.uptimeMillis()
    val view = makeView(tab)
    val transport = message.obj as? WebView.WebViewTransport ?: return false.also { close(tab) }
    transport.webView = view.page
    message.sendToTarget()
    changed(tab)
    return true
  }

  @SuppressLint("SetJavaScriptEnabled", "RestrictedApi")
  private fun makeView(tab: Tab): TabView {
    wantIcons()
    val page = PageView(activity, tab)
    if (profiles) {
      // Before anything else touches the page: a profile can't change later.
      val name = if (tab.isPrivate) PRIVATE_PROFILE else PROFILE
      ProfileStore.getInstance().getOrCreateProfile(name)
      WebViewCompat.setProfile(page, name)
    }
    with(page.settings) {
      javaScriptEnabled = true
      domStorageEnabled = true
      setSupportZoom(true)
      builtInZoomControls = true
      displayZoomControls = false
      useWideViewPort = true
      loadWithOverviewMode = true
      setSupportMultipleWindows(true)
      // Popups only from a tap, as Chrome allows them.
      javaScriptCanOpenWindowsAutomatically = false
      mediaPlaybackRequiresUserGesture = true
      // Pages never read the app's own files (the daemon's token lives there),
      // and a page opened from a file can't reach other files or other sites.
      allowFileAccess = false
      allowContentAccess = false
      @Suppress("DEPRECATION")
      allowFileAccessFromFileURLs = false
      @Suppress("DEPRECATION")
      allowUniversalAccessFromFileURLs = false
      setGeolocationEnabled(true)
      // A secure page never loads a part over plain HTTP: what it shows is
      // what came over the private connection.
      mixedContentMode = WebSettings.MIXED_CONTENT_NEVER_ALLOW
      textZoom = tab.textZoom
      if (tab.desktop) userAgentString = desktopAgent()
      if (tab.isPrivate && !profiles) {
        // No profile to keep a private tab apart: at least nothing is cached.
        cacheMode = WebSettings.LOAD_NO_CACHE
      }
    }
    // Chrome's defaults: third-party cookies in normal tabs, none in private ones.
    cookies(tab.isPrivate).setAcceptThirdPartyCookies(page, !tab.isPrivate)
    // The agent helper runs at the start of the pages agents may use. A
    // private tab gets none: it is handed to agents only on the user's word,
    // and then the helper is put in on the agent's first call (AgentHost.ready).
    if (!tab.isPrivate && WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)) {
      WebViewCompat.addDocumentStartJavaScript(page, scripts, setOf("*"))
    }
    page.webViewClient = PageClient(this, tab)
    page.webChromeClient = PageChrome(this, tab)
    page.setDownloadListener { url, userAgent, disposition, mime, length ->
      files.download(tab, url, userAgent, disposition, mime, length)
    }
    page.setFindListener { active, count, _ ->
      emit("find", JSObject().apply {
        put("tab", tab.id)
        put("count", count)
        put("active", if (count > 0) active + 1 else 0)
      })
    }
    page.onScrolled = { dy -> scrolled(tab, dy) }
    page.setOnLongClickListener { longPress(tab) }
    page.isFocusable = true
    page.isFocusableInTouchMode = true
    val view = TabView(activity, page)
    view.onRefresh = { reload(tab) }
    view.onTouch = { down -> touched(tab, down) }
    tab.view = view
    tab.crashed = false
    tab.used = SystemClock.uptimeMillis()
    view.layoutParams = FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
    place(tab)
    if (tab === shown) onstage(tab) else offstage(tab)
    trim()
    return view
  }

  /** WebView fetches pages' icons only once its (otherwise retired) icon store is opened. */
  private fun wantIcons() {
    if (iconsOn) return
    iconsOn = true
    try {
      val store = Class.forName("android.webkit.WebIconDatabase")
      val db = store.getMethod("getInstance").invoke(null)
      store.getMethod("open", String::class.java).invoke(db, activity.getDir("icons", Context.MODE_PRIVATE).path)
    } catch (_: Throwable) {
      // Without it tabs show the console's globe instead of the site's icon.
    }
  }

  fun cookies(isPrivate: Boolean): CookieManager =
    if (profiles) ProfileStore.getInstance().getOrCreateProfile(if (isPrivate) PRIVATE_PROFILE else PROFILE).cookieManager
    else CookieManager.getInstance()

  private fun storage(isPrivate: Boolean): WebStorage =
    if (profiles) ProfileStore.getInstance().getOrCreateProfile(if (isPrivate) PRIVATE_PROFILE else PROFILE).webStorage
    else WebStorage.getInstance()

  private fun locations(isPrivate: Boolean): GeolocationPermissions =
    if (profiles) ProfileStore.getInstance().getOrCreateProfile(if (isPrivate) PRIVATE_PROFILE else PROFILE).geolocationPermissions
    else GeolocationPermissions.getInstance()

  /** Brings back a tab that was put to sleep. */
  fun wake(tab: Tab) {
    if (tab.view != null) return
    val view = makeView(tab)
    val saved = tab.saved
    tab.saved = null
    if (saved == null || view.page.restoreState(saved) == null) {
      view.page.loadUrl(tab.url.ifEmpty { "about:blank" })
    }
    changed(tab)
  }

  /** Puts a live tab to sleep: its address and history stay, its page goes. */
  private fun sleep(tab: Tab) {
    val view = tab.view ?: return
    if (!tab.isPrivate) shots.thumbnailNow(tab)
    val state = Bundle()
    view.page.saveState(state)
    tab.saved = state
    (view.parent as? ViewGroup)?.removeView(view)
    view.page.destroy()
    tab.view = null
    changed(tab)
  }

  private fun trim() {
    val live = tabs.values.filter { it.view != null }
    if (live.size <= MAX_LIVE) return
    live.filter { it !== shown && !agentBusy(it) && fullscreen?.tab !== it }
      .sortedBy { it.used }
      .take(live.size - MAX_LIVE)
      .forEach { sleep(it) }
  }

  private fun sleepAllBut(keep: Int) {
    tabs.values.filter { it.view != null && it !== shown && !agentBusy(it) && fullscreen?.tab !== it }
      .sortedByDescending { it.used }
      .drop((keep - 1).coerceAtLeast(0))
      .forEach { sleep(it) }
  }

  fun close(tab: Tab) {
    if (tabs.remove(tab.id) == null) return
    if (shown === tab) {
      shown = null
      secure()
    }
    if (fullscreen?.tab === tab) exitFullscreen()
    permissions.dropTab(tab)
    tab.dialog?.result?.cancel()
    tab.dialog = null
    tab.view?.let { view ->
      (view.parent as? ViewGroup)?.removeView(view)
      view.page.stopLoading()
      view.page.destroy()
    }
    tab.view = null
    tab.saved = null
    emit("closed", JSObject().apply { put("id", tab.id) })
    if (tab.isPrivate && tabs.values.none { it.isPrivate }) forgetPrivate(0)
    updateBack()
  }

  /** A page closed its own window (window.close in a popup). */
  fun closedByPage(tab: Tab) {
    ui.post { close(tab) }
  }

  /** The last private tab closed: nothing it saw stays. */
  private fun forgetPrivate(attempt: Int) {
    if (!profiles || tabs.values.any { it.isPrivate }) return
    try {
      ProfileStore.getInstance().deleteProfile(PRIVATE_PROFILE)
    } catch (_: IllegalStateException) {
      // Still held by a page that is being torn down: try again shortly.
      if (attempt < 5) ui.postDelayed({ forgetPrivate(attempt + 1) }, 1500)
    } catch (_: Exception) {
    }
  }

  /** At start, a private profile left by a process that ended mid-session. */
  fun forgetLeftovers() {
    if (profiles && ProfileStore.getInstance().allProfileNames.contains(PRIVATE_PROFILE)) forgetPrivate(0)
  }

  // ---- where pages sit ----

  fun frameOf(bounds: JSONObject?): Frame? {
    if (bounds == null) return null
    val scale = bounds.optDouble("scale", Double.NaN).takeIf { !it.isNaN() && it > 0 }?.toFloat() ?: density
    val x = bounds.optDouble("x", 0.0)
    val y = bounds.optDouble("y", 0.0)
    val w = bounds.optDouble("width", 0.0)
    val h = bounds.optDouble("height", 0.0)
    // Edges rounded, not sizes, so the page meets the console's own pixels.
    val left = (x * scale).roundToInt()
    val top = (y * scale).roundToInt()
    val right = ((x + w) * scale).roundToInt()
    val bottom = ((y + h) * scale).roundToInt()
    val bar = (bounds.optDouble("bar", 0.0).takeIf { !it.isNaN() } ?: 0.0) * scale
    return Frame(
      left,
      top,
      (right - left).coerceAtLeast(0),
      (bottom - top).coerceAtLeast(0),
      bar.roundToInt().coerceAtLeast(0),
      scale,
      bounds.optBoolean("still", false),
      bounds.optBoolean("back", true),
    )
  }

  fun setFrame(tab: Tab, frame: Frame?) {
    val before = tab.frame
    tab.frame = frame
    if (tab === shown && (before?.bar != frame?.bar || frame?.parked == true)) resetBar(tab)
    place(tab)
    updateBack()
  }

  private fun place(tab: Tab) {
    val view = tab.view ?: return
    val lp = view.layoutParams as? FrameLayout.LayoutParams
      ?: FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
    lp.gravity = Gravity.TOP or Gravity.START
    val frame = tab.frame
    if (frame == null) {
      lp.width = ViewGroup.LayoutParams.MATCH_PARENT
      lp.height = ViewGroup.LayoutParams.MATCH_PARENT
      lp.leftMargin = 0
      lp.topMargin = 0
    } else {
      var height = frame.height
      // The keyboard took the bottom of the screen before the console said so.
      if (tab === shown && stage.height > 0 && !frame.parked) height = min(height, stage.height - frame.top).coerceAtLeast(0)
      if (tab === shown && grown) height += frame.bar
      lp.width = frame.width
      lp.height = height
      lp.leftMargin = frame.left
      lp.topMargin = frame.top
    }
    view.layoutParams = lp
  }

  private fun moveTo(view: View, layer: FrameLayout) {
    if (view.parent === layer) return
    (view.parent as? ViewGroup)?.removeView(view)
    layer.addView(view)
  }

  private fun onstage(tab: Tab) {
    val view = tab.view ?: return
    if (stage.parent == null) return
    moveTo(view, stage)
    view.visibility = View.VISIBLE
  }

  private fun offstage(tab: Tab) {
    val view = tab.view ?: return
    if (wings.parent == null) return
    moveTo(view, wings)
    view.translationY = 0f
    // Behind the console, a page Chromium thinks is visible keeps running;
    // one nobody works in is hidden, so it throttles.
    view.visibility = if (agentBusy(tab)) View.VISIBLE else View.INVISIBLE
  }

  fun show(tab: Tab, focus: Boolean) {
    val before = shown
    if (before != null && before !== tab) hide(before)
    shown = tab
    tab.used = SystemClock.uptimeMillis()
    if (tab.view == null && !tab.crashed) wake(tab)
    resetBar(tab)
    place(tab)
    onstage(tab)
    if (focus) tab.page?.requestFocus()
    claimBack()
    secure()
    trim()
  }

  fun hide(tab: Tab) {
    if (shown !== tab) return
    shown = null
    // A private page leaves no picture behind: the tab grid draws its letter.
    if (!tab.isPrivate) shots.thumbnailNow(tab)
    resetBar(tab)
    offstage(tab)
    updateBack()
    secure()
  }

  /**
   * While a private page is on screen the window is secure: the phone takes
   * no picture of it for its recent apps (a picture it would keep on disk),
   * and screenshots of it are refused, as Chrome does for its incognito tabs.
   */
  private fun secure() {
    val window = activity.window ?: return
    if (shown?.isPrivate == true) window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
    else window.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
  }

  /** Where the page is on screen right now (for a picture taken from the screen). */
  fun onScreen(tab: Tab): Boolean {
    val view = tab.view ?: return false
    return tab === shown && view.parent === stage && view.isShown && tab.frame?.parked != true && view.width > 0
  }

  // ---- agents ----

  fun agentBusy(tab: Tab): Boolean =
    tab.controller != null || SystemClock.uptimeMillis() - tab.agentAt < AGENT_LINGER

  /** An agent is acting in the tab: wake it, and keep it rendering behind the console. */
  fun busy(tab: Tab) {
    tab.agentAt = SystemClock.uptimeMillis()
    ensureAttached()
    if (tab.view == null && !tab.crashed) wake(tab)
    if (tab !== shown) offstage(tab)
    ui.removeCallbacks(sweep)
    ui.postDelayed(sweep, AGENT_LINGER + 1000)
  }

  /** Who holds the tab, as the daemon says (null: nobody now). */
  fun mark(tab: Tab, controller: JSONObject?) {
    tab.controller = controller
    if (controller == null) {
      // The agent's hold ended: a dialog it never answered goes away.
      tab.dialog?.result?.cancel()
      tab.dialog = null
    } else {
      tab.agentAt = SystemClock.uptimeMillis()
    }
    if (tab !== shown) offstage(tab)
    changed(tab)
  }

  private fun sweepAgents() {
    var busyLeft = false
    for (tab in tabs.values) {
      if (tab === shown || tab.view == null) continue
      offstage(tab)
      if (agentBusy(tab)) busyLeft = true
    }
    if (busyLeft) ui.postDelayed(sweep, 30_000)
    trim()
  }

  // ---- page events ----

  fun pageStarted(tab: Tab) {
    permissions.dropTab(tab)
    if (tab === shown && offset > 0) easeBar(tab, 0)
    changed(tab)
  }

  fun historyChanged(tab: Tab) {
    val page = tab.page
    if (page != null) {
      tab.canBack = page.canGoBack()
      tab.canForward = page.canGoForward()
    }
    if (tab === shown) updateBack()
    changed(tab)
  }

  fun progress(tab: Tab, value: Int) {
    tab.progress = value
    // Steps of a tenth are enough for the bar, and keep the channel quiet.
    if (value == 100 || value - tab.sentProgress >= 10 || value < tab.sentProgress) {
      tab.sentProgress = value
      changed(tab)
    }
  }

  fun favicon(tab: Tab, icon: Bitmap) {
    val side = (32 * density).roundToInt().coerceIn(16, 96)
    val scaled = if (icon.width > side || icon.height > side) Bitmap.createScaledBitmap(icon, side, side, true) else icon
    val out = ByteArrayOutputStream()
    scaled.compress(Bitmap.CompressFormat.PNG, 100, out)
    tab.favicon = "data:image/png;base64," + Base64.encodeToString(out.toByteArray(), Base64.NO_WRAP)
    changed(tab)
  }

  fun problem(tab: Tab, text: String) {
    tab.problem = text
    tab.loading = false
    changed(tab)
  }

  /** The page's renderer went away: drop the dead page; the console offers a reload. */
  fun crashed(tab: Tab, dead: WebView) {
    val view = tab.view
    if (view != null && view.page === dead) {
      (view.parent as? ViewGroup)?.removeView(view)
      dead.destroy()
      tab.view = null
    }
    tab.crashed = true
    tab.loading = false
    tab.dialog = null
    if (fullscreen?.tab === tab) exitFullscreen()
    permissions.dropTab(tab)
    changed(tab)
    updateBack()
  }

  /**
   * A page is about to leave for an address: web addresses stay in the tab;
   * an app's address (mail, the phone, a store, an intent) opens that app,
   * only after a tap and only from the page itself, as in Chrome. True when
   * the tab should not load it.
   */
  fun leaving(tab: Tab, uri: Uri, gesture: Boolean, mainFrame: Boolean): Boolean {
    when (uri.scheme?.lowercase()) {
      "http", "https" -> return uri.host?.lowercase()?.trimEnd('.')?.endsWith(".localhost") == true
      "about", "data", "blob" -> return false
      null, "javascript", "file", "content" -> return true
    }
    if (!mainFrame || !gesture) return true
    files.openApp(tab, uri)
    return true
  }

  /** A page's alert, confirm or prompt: the agent's to answer while one works in the tab. */
  fun dialog(tab: Tab, kind: String, message: String?, value: String?, result: JsResult): Boolean {
    if (tab.controller == null) return false // the WebView shows its own dialog to the person
    tab.dialog?.result?.cancel()
    tab.dialog = PageDialog(kind, message.orEmpty(), value, result)
    return true
  }

  private fun longPress(tab: Tab): Boolean {
    val page = tab.page ?: return false
    val hit = page.hitTestResult
    val kind = when (hit.type) {
      WebView.HitTestResult.SRC_ANCHOR_TYPE -> "link"
      WebView.HitTestResult.IMAGE_TYPE -> "image"
      WebView.HitTestResult.SRC_IMAGE_ANCHOR_TYPE -> "image"
      else -> return false
    }
    val target = hit.extra ?: return false
    page.performHapticFeedback(HapticFeedbackConstants.LONG_PRESS)
    val payload = JSObject().apply {
      put("tab", tab.id)
      put("kind", kind)
      put("url", target)
    }
    if (hit.type == WebView.HitTestResult.SRC_IMAGE_ANCHOR_TYPE) {
      // A picture inside a link: the link's own address comes separately.
      val answer = Handler(Looper.getMainLooper()) { message ->
        message.data.getString("url")?.let { payload.put("link", it) }
        emit("context", payload)
        true
      }
      page.requestFocusNodeHref(answer.obtainMessage())
    } else {
      if (kind == "link") payload.put("link", target)
      emit("context", payload)
    }
    return true
  }

  // ---- the console's bar ----

  private fun scrolled(tab: Tab, dy: Int) {
    if (tab !== shown || fullscreen != null) return
    val bar = tab.frame?.bar ?: 0
    if (bar <= 0 || tab.frame?.parked == true) return
    snap?.cancel()
    setOffset(tab, (offset + dy).coerceIn(0, bar))
    ui.removeCallbacks(settle)
    ui.postDelayed(settle, 160)
  }

  private fun touched(tab: Tab, down: Boolean) {
    if (tab !== shown) return
    touching = down
    ui.removeCallbacks(settle)
    if (down) snap?.cancel() else ui.postDelayed(settle, 160)
  }

  private fun setOffset(tab: Tab, value: Int) {
    if (value == offset) return
    if (value > 0 && !grown) {
      // Taller first, so the bottom never shows a gap while the page slides.
      grown = true
      place(tab)
    }
    offset = value
    tab.view?.translationY = -value.toFloat()
    sendChrome(tab)
  }

  /** The finger is off and the page has settled: the bar is all the way out or in. */
  private fun settleBar() {
    val tab = shown ?: return
    if (touching) return
    val bar = tab.frame?.bar ?: 0
    easeBar(tab, if (bar > 0 && offset * 2 >= bar) bar else 0)
  }

  private fun easeBar(tab: Tab, target: Int) {
    snap?.cancel()
    if (target == offset) return finishBar(tab)
    if (tab.frame?.still == true || !ValueAnimator.areAnimatorsEnabled()) {
      setOffset(tab, target)
      return finishBar(tab)
    }
    snap = ValueAnimator.ofInt(offset, target).apply {
      duration = 140
      interpolator = DecelerateInterpolator()
      addUpdateListener { if (shown === tab) setOffset(tab, it.animatedValue as Int) }
      addListener(object : AnimatorListenerAdapter() {
        override fun onAnimationEnd(animation: Animator) {
          if (shown === tab) finishBar(tab)
        }
      })
      start()
    }
  }

  /** Back to the page's own height once the bar is fully in. */
  private fun finishBar(tab: Tab) {
    if (offset == 0 && grown) {
      grown = false
      place(tab)
    }
  }

  private fun resetBar(tab: Tab) {
    snap?.cancel()
    ui.removeCallbacks(settle)
    val moved = offset != 0
    offset = 0
    tab.view?.translationY = 0f
    if (grown) {
      grown = false
      place(tab)
    }
    if (moved) sendChrome(tab)
  }

  /** Tells the console where its bar is, once a frame at most. */
  private fun sendChrome(tab: Tab) {
    if (chromeQueued) return
    chromeQueued = true
    Choreographer.getInstance().postFrameCallback {
      chromeQueued = false
      val scale = tab.frame?.scale ?: density
      emit("chrome", JSObject().apply {
        put("tab", tab.id)
        put("offset", offset / scale)
      })
    }
  }

  // ---- fullscreen ----

  fun enterFullscreen(tab: Tab, view: View, callback: WebChromeClient.CustomViewCallback) {
    if (fullscreen != null) {
      callback.onCustomViewHidden()
      return
    }
    val window = activity.window
    val decor = window.decorView as ViewGroup
    view.setBackgroundColor(Color.BLACK)
    view.keepScreenOn = true
    decor.addView(view, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
    if (Build.VERSION.SDK_INT >= 28) {
      window.attributes = window.attributes.apply {
        layoutInDisplayCutoutMode = WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES
      }
    }
    WindowInsetsControllerCompat(window, decor).apply {
      systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
      hide(WindowInsetsCompat.Type.systemBars())
    }
    fullscreen = Fullscreen(tab, view, callback)
    tab.fullscreen = true
    changed(tab)
    emit("fullscreen", JSObject().apply {
      put("tab", tab.id)
      put("on", true)
    })
    claimBack()
  }

  fun exitFullscreen() {
    val full = fullscreen ?: return
    fullscreen = null
    val window = activity.window
    val decor = window.decorView as ViewGroup
    decor.removeView(full.view)
    if (Build.VERSION.SDK_INT >= 28) {
      window.attributes = window.attributes.apply {
        layoutInDisplayCutoutMode = WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_DEFAULT
      }
    }
    WindowInsetsControllerCompat(window, decor).show(WindowInsetsCompat.Type.systemBars())
    full.tab.fullscreen = false
    changed(full.tab)
    emit("fullscreen", JSObject().apply {
      put("tab", full.tab.id)
      put("on", false)
    })
    updateBack()
  }

  // ---- what the console asks ----

  fun navigate(tab: Tab, url: String) {
    if (tab.view == null) {
      tab.saved = null
      tab.url = url
      wake(tab)
      return
    }
    tab.page?.loadUrl(url)
  }

  fun reload(tab: Tab) {
    if (tab.view == null) {
      // A crashed page starts over; a sleeping one wakes (which loads it).
      tab.crashed = false
      tab.saved = null
      wake(tab)
      if (tab === shown) show(tab, false)
      return
    }
    tab.problem = null
    tab.page?.reload()
  }

  fun stop(tab: Tab) {
    tab.page?.stopLoading()
    tab.loading = false
    changed(tab)
  }

  fun zoom(tab: Tab, factor: Double) {
    // The phone's zoom is text size, as Chrome's on Android; pages also pinch.
    tab.textZoom = (factor * 100).roundToInt().coerceIn(50, 300)
    tab.page?.settings?.textZoom = tab.textZoom
    changed(tab)
  }

  fun find(tab: Tab, term: String, step: Int) {
    val page = tab.page ?: return
    when {
      term.isEmpty() -> {
        tab.findTerm = null
        page.clearMatches()
      }
      term == tab.findTerm && step != 0 -> page.findNext(step > 0)
      else -> {
        tab.findTerm = term
        page.findAllAsync(term)
      }
    }
  }

  /** Asks sites for their desktop pages (or the phone's again), and reloads. */
  fun desktop(tab: Tab, on: Boolean) {
    tab.desktop = on
    tab.page?.let { page ->
      page.settings.userAgentString = if (on) desktopAgent() else null
      page.reload()
    }
    changed(tab)
  }

  /**
   * The engine's own user agent as a desktop build of it words its platform.
   * The WebView token stays, so sites that refuse embedded browsers (Google
   * sign-in) still know what this is: no disguise.
   */
  private fun desktopAgent(): String {
    val engine = Regex("""AppleWebKit/[\d.]+ \(KHTML, like Gecko\)""").find(defaultAgent)?.value
      ?: "AppleWebKit/537.36 (KHTML, like Gecko)"
    val chrome = Regex("""Chrome/[\d.]+""").find(defaultAgent)?.value ?: "Chrome/120.0.0.0"
    return "Mozilla/5.0 (X11; Linux x86_64; wv) $engine $chrome Safari/537.36"
  }

  /** A private tab the user hands to agents (or takes back). */
  fun shareWithAgents(tab: Tab, shared: Boolean) {
    if (!tab.isPrivate) return
    tab.shared = shared
    if (!shared) mark(tab, null)
    changed(tab)
  }

  /** Forgets the current site: its cookies and storage. */
  fun clearSite(tab: Tab): String? {
    val uri = Uri.parse(tab.page?.url ?: tab.url)
    val host = uri.host ?: return "this page has no site to clear"
    if (uri.scheme != "http" && uri.scheme != "https") return "this page has no site to clear"
    val origin = "${uri.scheme}://$host${if (uri.port != -1) ":${uri.port}" else ""}"
    storage(tab.isPrivate).deleteOrigin(origin)
    val jar = cookies(tab.isPrivate)
    expireCookies(jar, "${uri.scheme}://$host/", host)
    jar.flush()
    tab.page?.reload()
    return null
  }

  /** CookieManager can't remove one site's cookies, so each one is expired. */
  private fun expireCookies(jar: CookieManager, url: String, host: String) {
    val names = jar.getCookie(url)?.split(';')?.mapNotNull { it.substringBefore('=').trim().takeIf { n -> n.isNotEmpty() } }
      ?: return
    val domains = mutableListOf<String?>(null)
    val parts = host.split('.')
    for (i in 0 until parts.size - 1) domains.add("." + parts.drop(i).joinToString("."))
    for (name in names) {
      for (domain in domains) {
        val scope = if (domain != null) "; Domain=$domain" else ""
        jar.setCookie(url, "$name=; Max-Age=0; Path=/$scope")
      }
    }
  }

  /**
   * Clears what the browser keeps, by kind: "cookies", "site_data" (sites'
   * storage), "cache", "permissions" (sites' location answers). Browsing
   * history and bookmarks are the daemon's, cleared through its routes.
   */
  fun clearData(kinds: Set<String>) {
    val all = "all" in kinds
    val wantCookies = all || kinds.any { it in setOf("cookies", "cookies_and_site_data") }
    val wantStorage = all || kinds.any { it in setOf("site_data", "storage", "cookies_and_site_data") }
    val wantCache = all || kinds.any { it in setOf("cache", "cached") }
    val wantLocations = all || "permissions" in kinds
    for (isPrivate in listOf(false, true)) {
      if (isPrivate && !profiles) continue
      if (wantCookies) cookies(isPrivate).run {
        removeAllCookies(null)
        flush()
      }
      if (wantStorage) clearStorage(isPrivate)
      if (wantLocations) locations(isPrivate).clearAll()
    }
    if (wantCache) clearCache()
  }

  private fun clearStorage(isPrivate: Boolean) {
    val store = storage(isPrivate)
    if (profiles) {
      store.deleteAllData()
      return
    }
    // Without profiles the console's own storage is in the same place:
    // only the sites' goes.
    store.getOrigins { origins ->
      val map = origins as? Map<*, *> ?: return@getOrigins
      for (key in map.keys) {
        val origin = key?.toString() ?: continue
        val host = Uri.parse(origin).host ?: continue
        if (host !in CONSOLE_HOSTS) store.deleteOrigin(origin)
      }
    }
  }

  /** The cache is per profile and only a page can clear it: a live one, or a passing one. */
  @SuppressLint("RestrictedApi")
  private fun clearCache() {
    for (isPrivate in listOf(false, true)) {
      if (isPrivate && !profiles) continue
      val live = tabs.values.firstOrNull { it.isPrivate == isPrivate && it.page != null }?.page
      if (live != null) {
        live.clearCache(true)
        continue
      }
      val passing = WebView(activity)
      if (profiles) {
        val name = if (isPrivate) PRIVATE_PROFILE else PROFILE
        ProfileStore.getInstance().getOrCreateProfile(name)
        WebViewCompat.setProfile(passing, name)
      }
      passing.clearCache(true)
      passing.destroy()
    }
  }

  /** Every tab, oldest first, for a console that reloaded. */
  fun tabList(): JSONArray = JSONArray().apply { for (tab in tabs.values) put(tabJson(tab)) }
}

/**
 * The page-side half of the agent bridge, run at every page's start before
 * the agent helper (openremote-browser's agent.js): a place for answers to
 * calls that finish later, since evaluateJavascript can't wait for one.
 * Android's WebView has no isolated worlds, so this lives in the page's
 * world under a name it can't overwrite, holding the built-ins it needs
 * from before the page's scripts run.
 */
internal val HOST_SCRIPT = """
(() => {
  if (globalThis.__openremote_host) return
  const slots = new Map()
  const set = Map.prototype.set
  const get = Map.prototype.get
  const has = Map.prototype.has
  const del = Map.prototype.delete
  const toJson = JSON.stringify
  const fromJson = JSON.parse
  const pending = Object.freeze({ pending: true })
  const plain = (v) => {
    if (v === undefined) return null
    try {
      const text = toJson(v)
      return text === undefined ? String(v) : fromJson(text)
    } catch (e) {
      return String(v)
    }
  }
  const host = {
    run(id, fn) {
      set.call(slots, id, pending)
      Promise.resolve()
        .then(fn)
        .then(
          (v) => set.call(slots, id, { ok: plain(v), none: v === undefined }),
          (e) => set.call(slots, id, { error: String((e && e.message) || e) }),
        )
    },
    take(id) {
      if (!has.call(slots, id)) return null
      const v = get.call(slots, id)
      if (v !== pending) del.call(slots, id)
      return v
    },
    keep(id, value) {
      set.call(slots, id, value)
    },
    slice(id, from, size) {
      const v = get.call(slots, id)
      return typeof v === 'string' ? v.slice(from, from + size) : null
    },
    drop(id) {
      del.call(slots, id)
    },
  }
  Object.defineProperty(globalThis, '__openremote_host', { value: Object.freeze(host) })
})()
""".trimIndent()
