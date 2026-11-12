// Pictures of pages. The console shows one in the page's place while its
// own menus cover the page (a native view always draws above the console),
// the tab grid shows small ones, and agents read pages as pictures too. A
// page on screen is copied from the screen itself (video and canvases
// included); one out of sight is drawn into a bitmap.

package space.moreweb.openremote.browser

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Rect
import android.util.Base64
import android.view.PixelCopy
import android.webkit.WebView
import java.io.ByteArrayOutputStream
import java.util.concurrent.Executors
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

/** A part of the page, in its CSS pixels. */
class Clip(val x: Double, val y: Double, val width: Double, val height: Double)

class Shots(private val browser: Browser) {
  private val work = Executors.newSingleThreadExecutor()

  /**
   * The page as it looks now, at most [maxWidth] pixels wide: from the
   * screen when it is on screen and [fromScreen] allows, else drawn.
   */
  fun bitmap(tab: Tab, maxWidth: Int, fromScreen: Boolean, done: (Bitmap?) -> Unit) {
    val page = tab.page ?: return done(null)
    if (page.width <= 0 || page.height <= 0) return done(null)
    if (!fromScreen || !browser.onScreen(tab)) return done(draw(page, maxWidth))
    val at = IntArray(2)
    page.getLocationInWindow(at)
    val window = browser.activity.window
    val decor = window.decorView
    val area = Rect(at[0], at[1], at[0] + page.width, at[1] + page.height)
    if (!area.intersect(0, 0, decor.width, decor.height) || area.isEmpty) return done(draw(page, maxWidth))
    val copy = try {
      Bitmap.createBitmap(area.width(), area.height(), Bitmap.Config.ARGB_8888)
    } catch (_: OutOfMemoryError) {
      return done(null)
    }
    try {
      PixelCopy.request(window, area, copy, { result ->
        if (result == PixelCopy.SUCCESS) {
          done(fit(copy, maxWidth))
        } else {
          copy.recycle()
          done(draw(page, maxWidth))
        }
      }, browser.ui)
    } catch (_: Exception) {
      copy.recycle()
      done(draw(page, maxWidth))
    }
  }

  /** The visible part of a page, drawn by the page itself into a bitmap. */
  fun draw(page: WebView, maxWidth: Int): Bitmap? = try {
    val scale = min(1f, maxWidth.toFloat() / page.width)
    val bitmap = Bitmap.createBitmap(
      max(1, (page.width * scale).roundToInt()),
      max(1, (page.height * scale).roundToInt()),
      Bitmap.Config.ARGB_8888,
    )
    val canvas = Canvas(bitmap)
    canvas.scale(scale, scale)
    // A view draws its content where its scroll puts it.
    canvas.translate(-page.scrollX.toFloat(), -page.scrollY.toFloat())
    page.draw(canvas)
    bitmap
  } catch (_: Throwable) {
    null
  }

  fun fit(bitmap: Bitmap, maxWidth: Int): Bitmap {
    if (bitmap.width <= maxWidth) return bitmap
    val scaled = Bitmap.createScaledBitmap(
      bitmap,
      maxWidth,
      max(1, (bitmap.height * maxWidth.toFloat() / bitmap.width).roundToInt()),
      true,
    )
    bitmap.recycle()
    return scaled
  }

  /** Part of a picture of the page: [clip] in CSS pixels, [scale] the page's pixels per CSS pixel. */
  fun crop(bitmap: Bitmap, pageWidth: Int, clip: Clip, scale: Float): Bitmap? {
    val k = scale * bitmap.width / pageWidth.toFloat()
    val left = (clip.x * k).roundToInt().coerceIn(0, bitmap.width - 1)
    val top = (clip.y * k).roundToInt().coerceIn(0, bitmap.height - 1)
    val width = (clip.width * k).roundToInt().coerceIn(1, bitmap.width - left)
    val height = (clip.height * k).roundToInt().coerceIn(1, bitmap.height - top)
    return try {
      Bitmap.createBitmap(bitmap, left, top, width, height)
    } catch (_: Exception) {
      null
    }
  }

  /** The picture as base64 JPEG, made off the main thread; the bitmap is spent after. */
  fun jpeg(bitmap: Bitmap, quality: Int, done: (String?) -> Unit) {
    work.execute {
      val out = ByteArrayOutputStream()
      val made = try {
        bitmap.compress(Bitmap.CompressFormat.JPEG, quality, out)
      } catch (_: Exception) {
        false
      }
      bitmap.recycle()
      val data = if (made) Base64.encodeToString(out.toByteArray(), Base64.NO_WRAP) else null
      browser.ui.post { done(data) }
    }
  }

  /**
   * browser_capture: the tab as a JPEG data URL, all of it or [clip],
   * at most [maxCss] CSS pixels wide. A sleeping tab answers with the
   * picture it had when it went to sleep.
   */
  fun capture(tab: Tab, clip: Clip?, maxCss: Double?, done: (String?) -> Unit) {
    val page = tab.page
    if (page == null) return done(if (tab.isPrivate) null else tab.thumbnail)
    // A thumbnail of a page out of sight: the one taken as it left the
    // screen (a hidden page may draw blank).
    if (maxCss != null && clip == null && !browser.onScreen(tab) && tab.thumbnail != null) return done(tab.thumbnail)
    val scale = tab.frame?.scale ?: browser.density
    val widest = maxCss?.takeIf { it > 0 }?.let { (it * scale).roundToInt() } ?: page.width
    // A small picture is a thumbnail: it needn't be sharp.
    val quality = if (widest < page.width / 2) 60 else 80
    bitmap(tab, if (clip != null) page.width else widest, true) { shot ->
      if (shot == null) return@bitmap done(null)
      var picture: Bitmap = shot
      if (clip != null) {
        val part = crop(shot, page.width, clip, scale)
        if (part != null && part !== shot) {
          shot.recycle()
          picture = fit(part, widest)
        }
      }
      jpeg(picture, quality) { data -> done(data?.let { "data:image/jpeg;base64,$it" }) }
    }
  }

  /** A small picture for the tab grid, taken as the page leaves the screen. */
  fun thumbnailNow(tab: Tab) {
    // Never for a private tab: nothing it showed is kept, not even a picture.
    if (tab.isPrivate) return
    val page = tab.page ?: return
    if (page.width <= 0) return
    val shot = draw(page, (200 * browser.density).roundToInt()) ?: return
    jpeg(shot, 60) { data -> if (data != null) tab.thumbnail = "data:image/jpeg;base64,$data" }
  }
}
