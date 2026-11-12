// Sites asking for the camera, the microphone, the location and the like.
// Each request waits as a `browser:permission` event the console answers
// (browser_permission), and nothing is granted until it does: denied by
// default, as on the desktop. Allowing also needs the phone's own
// permission for the app, asked the first time a site needs it.

package space.moreweb.openremote.browser

import android.Manifest
import android.content.pm.PackageManager
import android.net.Uri
import android.webkit.GeolocationPermissions
import android.webkit.PermissionRequest
import androidx.core.content.ContextCompat
import app.tauri.plugin.JSObject
import app.tauri.plugin.PluginManager

class Permissions(private val browser: Browser) {
  private class Ask(
    val id: Long,
    val tab: Tab,
    val origin: String,
    val kinds: List<String>,
    val request: PermissionRequest?,
    val location: GeolocationPermissions.Callback?,
  )

  private var next = 0L
  private val waiting = LinkedHashMap<Long, Ask>()

  fun ask(tab: Tab, request: PermissionRequest) {
    val kinds = request.resources.mapNotNull { KINDS[it] }.distinct()
    if (kinds.isEmpty()) {
      request.deny()
      return
    }
    queue(Ask(++next, tab, origin(request.origin), kinds, request, null))
  }

  fun askLocation(tab: Tab, origin: String, callback: GeolocationPermissions.Callback) {
    queue(Ask(++next, tab, origin.trimEnd('/'), listOf("geolocation"), null, callback))
  }

  private fun queue(ask: Ask) {
    waiting[ask.id] = ask
    browser.emit("permission", json(ask, false))
  }

  fun cancelled(request: PermissionRequest) {
    val ask = waiting.values.firstOrNull { it.request === request } ?: return
    waiting.remove(ask.id)
    browser.emit("permission", json(ask, true))
  }

  fun locationGone(tab: Tab) {
    for (ask in waiting.values.filter { it.tab === tab && it.location != null }) {
      waiting.remove(ask.id)
      browser.emit("permission", json(ask, true))
    }
  }

  /** The page that asked is going (a new page, a closed tab): its requests end, denied. */
  fun dropTab(tab: Tab) {
    for (ask in waiting.values.filter { it.tab === tab }) {
      waiting.remove(ask.id)
      deny(ask)
      browser.emit("permission", json(ask, true))
    }
  }

  /** The console's answer. [done] hears a problem in words, or null. */
  fun answer(id: Long, allow: Boolean, done: (String?) -> Unit) {
    val ask = waiting.remove(id) ?: return done("that request is no longer waiting")
    if (!allow) {
      deny(ask)
      return done(null)
    }
    val activity = browser.activity
    val missing = ask.kinds.flatMap { ANDROID[it].orEmpty() }.distinct().filter {
      ContextCompat.checkSelfPermission(activity, it) != PackageManager.PERMISSION_GRANTED
    }
    if (missing.isEmpty()) {
      grant(ask, ask.kinds)
      return done(null)
    }
    try {
      PluginManager.requestPermissions(missing.toTypedArray()) { results ->
        // A kind is usable when the phone allows the app any of what it needs
        // (approximate location is enough for a site asking where you are).
        val usable = ask.kinds.filter { kind ->
          val needs = ANDROID[kind].orEmpty()
          needs.isEmpty() || needs.any {
            results[it] == true || ContextCompat.checkSelfPermission(activity, it) == PackageManager.PERMISSION_GRANTED
          }
        }
        if (usable.isEmpty()) {
          deny(ask)
          done("the phone didn't allow OpenRemote to ${label(ask.kinds)}")
        } else {
          grant(ask, usable)
          done(null)
        }
      }
    } catch (e: Exception) {
      deny(ask)
      done(e.message ?: "the phone couldn't ask for that permission")
    }
  }

  private fun grant(ask: Ask, kinds: List<String>) {
    ask.request?.let { request ->
      val resources = request.resources.filter { KINDS[it] in kinds }.toTypedArray()
      if (resources.isEmpty()) request.deny() else request.grant(resources)
    }
    // Remembering is the console's (the daemon keeps a site's answers), so
    // WebView is told not to retain this one.
    ask.location?.invoke(ask.origin, "geolocation" in kinds, false)
  }

  private fun deny(ask: Ask) {
    ask.request?.deny()
    ask.location?.invoke(ask.origin, false, false)
  }

  private fun json(ask: Ask, gone: Boolean) = JSObject().apply {
    put("tab", ask.tab.id)
    put("id", ask.id)
    put("origin", ask.origin)
    put("kind", ask.kinds.joinToString(","))
    put("label", label(ask.kinds))
    put("gone", gone)
  }

  private fun origin(uri: Uri): String {
    val port = if (uri.port != -1) ":${uri.port}" else ""
    return "${uri.scheme}://${uri.host}$port"
  }

  companion object {
    private val KINDS = mapOf(
      PermissionRequest.RESOURCE_VIDEO_CAPTURE to "camera",
      PermissionRequest.RESOURCE_AUDIO_CAPTURE to "microphone",
      PermissionRequest.RESOURCE_PROTECTED_MEDIA_ID to "protected-media",
      PermissionRequest.RESOURCE_MIDI_SYSEX to "midi",
    )

    /** What the app itself needs from the phone for each kind. */
    private val ANDROID = mapOf(
      "camera" to listOf(Manifest.permission.CAMERA),
      "microphone" to listOf(Manifest.permission.RECORD_AUDIO),
      "geolocation" to listOf(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION),
    )

    private val WORDS = mapOf(
      "camera" to "use your camera",
      "microphone" to "use your microphone",
      "geolocation" to "know your location",
      "protected-media" to "play protected content",
      "midi" to "use your MIDI devices",
    )

    /** The request in plain words, finishing "example.com wants to ...". */
    fun label(kinds: List<String>): String {
      if (kinds.toSet() == setOf("camera", "microphone")) return "use your camera and microphone"
      return kinds.joinToString(" and ") { WORDS[it] ?: it }
    }
  }
}
