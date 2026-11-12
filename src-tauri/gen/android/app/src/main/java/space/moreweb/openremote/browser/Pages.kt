// One tab of the built-in browser: what the console is told about it, the
// page itself (a WebView of its own), the frame holding the page with its
// pull-to-refresh mark, and the clients that report what the page does.
// Browser.kt owns the tabs and decides where each page sits.

package space.moreweb.openremote.browser

import android.annotation.SuppressLint
import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.RectF
import android.net.Uri
import android.net.http.SslError
import android.os.Bundle
import android.os.Message
import android.os.SystemClock
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.webkit.ConsoleMessage
import android.webkit.GeolocationPermissions
import android.webkit.JsPromptResult
import android.webkit.JsResult
import android.webkit.PermissionRequest
import android.webkit.RenderProcessGoneDetail
import android.webkit.SslErrorHandler
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.FrameLayout
import org.json.JSONObject
import kotlin.math.min

// The app's own colours (DESIGN.md), for the little the browser draws itself.
internal const val ACCENT = 0xFFDCA24B.toInt()
internal const val TEXT_2 = 0xFF9AA3AE.toInt()
internal const val RAISED = 0xFF171B21.toInt()
internal const val LINE = 0x12FFFFFF

/** Where the console's page area is, in the layer's pixels. */
class Frame(
  val left: Int,
  val top: Int,
  val width: Int,
  val height: Int,
  /** How far the page may slide up under the console's bar while scrolling. */
  val bar: Int,
  /** Physical pixels per CSS pixel of the console. */
  val scale: Float,
  /** No motion: the bar snaps instead of easing (reduced motion). */
  val still: Boolean,
  /** Android's back goes to the page's history; false while the console needs it. */
  val back: Boolean,
) {
  /** The console moved the page out of sight while one of its menus covers it. */
  val parked: Boolean get() = left + width <= 0 || top + height <= 0
}

/** Lines a page wrote (its console, its requests), kept until an agent reads them. */
class Ring(private val max: Int) {
  private val lines = ArrayDeque<String>()

  @Synchronized
  fun add(line: String) {
    if (lines.size >= max) lines.removeFirst()
    lines.addLast(if (line.length > 2000) line.take(1999) + "…" else line)
  }

  @Synchronized
  fun drain(): List<String> {
    val out = lines.toList()
    lines.clear()
    return out
  }
}

/** A page's alert, confirm or prompt, held for the agent working in the tab. */
class PageDialog(val kind: String, val message: String, val value: String?, val result: JsResult)

class Tab(val id: String, val isPrivate: Boolean, var opener: String?) {
  var url = ""
  var title = ""
  var loading = false
  var progress = 0
  var sentProgress = 0
  /** The page's icon as a data URL: the console shows no remote images. */
  var favicon: String? = null
  var canBack = false
  var canForward = false
  /** Who holds the tab, as the daemon said (its controller): the console badges it. */
  var controller: JSONObject? = null
  /** A private tab the user let agents use. */
  var shared = false
  var fullscreen = false
  /** Asks sites for their desktop pages. */
  var desktop = false
  /** The page's renderer went away; the console offers a reload. */
  var crashed = false
  var textZoom = 100
  /** Something the person should know about the page, in plain words. */
  var problem: String? = null
  /** The live page, or null while discarded (or crashed). */
  var view: TabView? = null
  /** The page's history while discarded: in memory only, never written to disk. */
  var saved: Bundle? = null
  var thumbnail: String? = null
  var frame: Frame? = null
  var used = SystemClock.uptimeMillis()
  /** When an agent last acted here: its page keeps rendering out of sight. */
  var agentAt = 0L
  val consoleLines = Ring(200)
  val networkLines = Ring(300)
  var dialog: PageDialog? = null
  var findTerm: String? = null
  /** Main-frame loads started and finished: agents wait for their own. */
  var loads = 0
  var finished = 0

  val page: PageView? get() = view?.page

  /** Agents see every normal tab, and private ones only once handed over. */
  val forAgents: Boolean get() = !isPrivate || shared
}

/** The page: a plain WebView that also tells when it hits its top or scrolls. */
@SuppressLint("ViewConstructor")
class PageView(context: Context, val tab: Tab) : WebView(context) {
  /** The page met its top while a finger pulled down: a pull to refresh may begin. */
  var atTop = false
  var onScrolled: ((Int) -> Unit)? = null

  override fun onOverScrolled(scrollX: Int, scrollY: Int, clampedX: Boolean, clampedY: Boolean) {
    super.onOverScrolled(scrollX, scrollY, clampedX, clampedY)
    if (clampedY && scrollY <= 0) atTop = true
  }

  override fun onScrollChanged(l: Int, t: Int, oldl: Int, oldt: Int) {
    super.onScrollChanged(l, t, oldl, oldt)
    if (t != oldt) onScrolled?.invoke(t - oldt)
  }
}

/**
 * The frame a live page sits in. It watches the finger on its way to the
 * page (never taking the gesture from it), so a pull down past the page's
 * top reloads, the way Chrome does.
 */
@SuppressLint("ViewConstructor")
class TabView(context: Context, val page: PageView) : FrameLayout(context) {
  private val density = resources.displayMetrics.density
  private val mark = PullMark(context)
  private val reach = 72f * density
  private var downY = 0f
  private var from = Float.NaN
  private var pulled = 0f
  var onRefresh: (() -> Unit)? = null
  var onTouch: ((Boolean) -> Unit)? = null

  init {
    addView(page, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT))
    val size = (40 * density).toInt()
    addView(mark, LayoutParams(size, size, Gravity.TOP or Gravity.CENTER_HORIZONTAL))
    mark.visibility = View.GONE
  }

  override fun dispatchTouchEvent(ev: MotionEvent): Boolean {
    when (ev.actionMasked) {
      MotionEvent.ACTION_DOWN -> {
        page.atTop = false
        from = Float.NaN
        pulled = 0f
        downY = ev.y
        onTouch?.invoke(true)
      }
      MotionEvent.ACTION_POINTER_DOWN -> settle(false)
      MotionEvent.ACTION_MOVE -> if (ev.pointerCount == 1) {
        if (from.isNaN() && page.atTop && page.scrollY <= 0 && ev.y > downY) from = ev.y
        if (!from.isNaN()) {
          pulled = (ev.y - from).coerceAtLeast(0f)
          mark.show(pulled, reach)
        }
      }
      MotionEvent.ACTION_UP -> {
        settle(!from.isNaN() && pulled >= reach)
        onTouch?.invoke(false)
      }
      MotionEvent.ACTION_CANCEL -> {
        settle(false)
        onTouch?.invoke(false)
      }
    }
    return super.dispatchTouchEvent(ev)
  }

  private fun settle(refresh: Boolean) {
    from = Float.NaN
    pulled = 0f
    mark.hide()
    if (refresh) onRefresh?.invoke()
  }
}

/**
 * Pull to refresh's mark: a ring that fills as the finger travels and turns
 * amber once letting go will reload. It follows the finger and nothing else
 * moves it, so there is no animation to turn off.
 */
class PullMark(context: Context) : View(context) {
  private val density = resources.displayMetrics.density
  private val fill = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = RAISED }
  private val edge = Paint(Paint.ANTI_ALIAS_FLAG).apply {
    style = Paint.Style.STROKE
    strokeWidth = density
    color = LINE
  }
  private val arc = Paint(Paint.ANTI_ALIAS_FLAG).apply {
    style = Paint.Style.STROKE
    strokeWidth = 2.5f * density
    strokeCap = Paint.Cap.ROUND
  }
  private val box = RectF()
  private var progress = 0f

  fun show(pulled: Float, reach: Float) {
    progress = (pulled / reach).coerceIn(0f, 1f)
    visibility = VISIBLE
    translationY = min(pulled, reach * 1.4f) * 0.6f + 8 * density
    invalidate()
  }

  fun hide() {
    progress = 0f
    visibility = GONE
  }

  override fun onDraw(canvas: Canvas) {
    val r = width / 2f
    canvas.drawCircle(r, r, r - density, fill)
    canvas.drawCircle(r, r, r - density, edge)
    val inset = 12 * density
    box.set(inset, inset, width - inset, height - inset)
    arc.color = if (progress >= 1f) ACCENT else TEXT_2
    canvas.drawArc(box, -90f, if (progress >= 1f) 360f else 330f * progress, false, arc)
  }
}

/** What the page's navigation and network do. */
class PageClient(private val browser: Browser, private val tab: Tab) : WebViewClient() {
  override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean =
    browser.leaving(tab, request.url, request.hasGesture(), request.isForMainFrame)

  override fun onPageStarted(view: WebView, url: String?, favicon: Bitmap?) {
    tab.loads += 1
    tab.loading = true
    tab.problem = null
    if (url != null) tab.url = url
    browser.pageStarted(tab)
  }

  override fun onPageFinished(view: WebView, url: String?) {
    tab.loading = false
    tab.finished = tab.loads
    tab.url = view.url ?: url ?: tab.url
    view.title?.let { if (it.isNotEmpty()) tab.title = it }
    browser.historyChanged(tab)
  }

  override fun doUpdateVisitedHistory(view: WebView, url: String?, isReload: Boolean) {
    if (url != null) tab.url = url
    browser.historyChanged(tab)
  }

  override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? {
    // Called off the main thread: the ring is synchronized.
    val main = if (request.isForMainFrame) " (the page)" else ""
    tab.networkLines.add("${request.method} ${shorten(request.url.toString())}$main")
    return null
  }

  override fun onReceivedHttpError(view: WebView, request: WebResourceRequest, errorResponse: WebResourceResponse) {
    tab.networkLines.add("${request.method} ${shorten(request.url.toString())} -> ${errorResponse.statusCode}")
  }

  override fun onReceivedError(view: WebView, request: WebResourceRequest, error: WebResourceError) {
    tab.networkLines.add("${request.method} ${shorten(request.url.toString())} -> failed: ${error.description}")
  }

  override fun onReceivedSslError(view: WebView, handler: SslErrorHandler, error: SslError) {
    // As Chrome does, nothing loads over a connection that isn't private;
    // unlike Chrome there is no way past it.
    handler.cancel()
    if (error.url == tab.url || error.url == view.url || tab.loading) {
      browser.problem(tab, "This site's certificate isn't valid, so its page wasn't opened: the connection isn't private.")
    }
  }

  override fun onRenderProcessGone(view: WebView, detail: RenderProcessGoneDetail): Boolean {
    browser.crashed(tab, view)
    // Handled: the app carries on and the console offers a reload.
    return true
  }

  private fun shorten(url: String) = if (url.length > 300) url.take(299) + "…" else url
}

/** What the page asks of the browser around it. */
class PageChrome(private val browser: Browser, private val tab: Tab) : WebChromeClient() {
  override fun onProgressChanged(view: WebView, newProgress: Int) = browser.progress(tab, newProgress)

  override fun onReceivedTitle(view: WebView, title: String?) {
    tab.title = title.orEmpty()
    browser.changed(tab)
  }

  override fun onReceivedIcon(view: WebView, icon: Bitmap?) {
    if (icon != null) browser.favicon(tab, icon)
  }

  override fun onCreateWindow(view: WebView, isDialog: Boolean, isUserGesture: Boolean, resultMsg: Message): Boolean =
    browser.popup(tab, isUserGesture, resultMsg)

  override fun onCloseWindow(window: WebView) = browser.closedByPage(tab)

  override fun onShowCustomView(view: View, callback: CustomViewCallback) = browser.enterFullscreen(tab, view, callback)

  override fun onHideCustomView() = browser.exitFullscreen()

  override fun onPermissionRequest(request: PermissionRequest) = browser.permissions.ask(tab, request)

  override fun onPermissionRequestCanceled(request: PermissionRequest) = browser.permissions.cancelled(request)

  override fun onGeolocationPermissionsShowPrompt(origin: String, callback: GeolocationPermissions.Callback) =
    browser.permissions.askLocation(tab, origin, callback)

  override fun onGeolocationPermissionsHidePrompt() = browser.permissions.locationGone(tab)

  override fun onShowFileChooser(
    webView: WebView,
    filePathCallback: ValueCallback<Array<Uri>>,
    fileChooserParams: FileChooserParams,
  ): Boolean = browser.files.choose(filePathCallback, fileChooserParams)

  override fun onConsoleMessage(consoleMessage: ConsoleMessage): Boolean {
    val level = consoleMessage.messageLevel().name.lowercase()
    val where = consoleMessage.sourceId()?.substringAfterLast('/')?.takeIf { it.isNotEmpty() }
    val at = if (where != null) " ($where:${consoleMessage.lineNumber()})" else ""
    tab.consoleLines.add("[$level] ${consoleMessage.message()}$at")
    // Kept for agents; a page's console has no business in the system log.
    return true
  }

  override fun onJsAlert(view: WebView, url: String?, message: String?, result: JsResult): Boolean =
    browser.dialog(tab, "alert", message, null, result)

  override fun onJsConfirm(view: WebView, url: String?, message: String?, result: JsResult): Boolean =
    browser.dialog(tab, "confirm", message, null, result)

  override fun onJsPrompt(
    view: WebView,
    url: String?,
    message: String?,
    defaultValue: String?,
    result: JsPromptResult,
  ): Boolean = browser.dialog(tab, "prompt", message, defaultValue, result)

  // WebView's grey play-button poster for videos without one; Chrome shows none.
  override fun getDefaultVideoPoster(): Bitmap? = Bitmap.createBitmap(1, 1, Bitmap.Config.ARGB_8888)
}
