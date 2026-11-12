// Files in and out of pages: downloads land in the phone's Downloads (web
// addresses through Android's DownloadManager, with the page's cookies;
// blob: and data: addresses, which only the page can read, saved from the
// page), a page's file picker opens the phone's, and links to apps (mail,
// the phone, a store) open those apps.

package space.moreweb.openremote.browser

import android.app.DownloadManager
import android.content.ActivityNotFoundException
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import android.util.Base64
import android.webkit.MimeTypeMap
import android.webkit.URLUtil
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import androidx.core.content.FileProvider
import app.tauri.plugin.JSObject
import app.tauri.plugin.PluginManager
import org.json.JSONObject
import java.io.File
import java.net.URLDecoder
import java.util.concurrent.Executors

/** The most a page may hand over through script (a blob: download, a saved file). */
private const val MAX_FROM_PAGE = 64L * 1024 * 1024

class Files(private val browser: Browser) {
  private class Watch(
    val id: Long,
    val tab: String,
    val url: String,
    val name: String,
    val mime: String,
    val agents: Boolean,
    var received: Long = -1,
  )

  private val work = Executors.newSingleThreadExecutor()
  private val manager: DownloadManager
    get() = browser.activity.getSystemService(Context.DOWNLOAD_SERVICE) as DownloadManager
  private var next = 0L
  private val watching = LinkedHashMap<Long, Watch>()
  private val poll = Runnable { check() }

  fun download(tab: Tab, url: String, userAgent: String?, disposition: String?, mime: String?, length: Long) {
    val type = mime?.takeIf { it.isNotBlank() && it != "application/octet-stream" }
      ?: guessType(url) ?: mime ?: "application/octet-stream"
    val name = cleanName(URLUtil.guessFileName(url, disposition, type))
    when (Uri.parse(url).scheme?.lowercase()) {
      "http", "https" -> fromWeb(tab, url, userAgent, name, type)
      "data" -> fromData(tab, url, name)
      "blob" -> fromBlob(tab, url, name, type)
      else -> report(++next, tab, url, name, type, "failed", null, 0, length)
    }
  }

  /** A link or picture the person chose to download from its menu. */
  fun downloadLink(tab: Tab, url: String) {
    download(tab, url, tab.page?.settings?.userAgentString, null, null, -1)
  }

  private fun fromWeb(tab: Tab, url: String, userAgent: String?, name: String, mime: String) {
    val id = ++next
    try {
      val request = DownloadManager.Request(Uri.parse(url))
        .setMimeType(mime)
        .setTitle(name)
        .setNotificationVisibility(DownloadManager.Request.VISIBILITY_VISIBLE_NOTIFY_COMPLETED)
      // The page's own session: a download behind a sign-in still works.
      browser.cookies(tab.isPrivate).getCookie(url)?.let { request.addRequestHeader("Cookie", it) }
      userAgent?.let { request.addRequestHeader("User-Agent", it) }
      if (Build.VERSION.SDK_INT >= 29) {
        request.setDestinationInExternalPublicDir(Environment.DIRECTORY_DOWNLOADS, name)
      } else {
        // Before Android 10 the shared Downloads needs a storage permission
        // the app doesn't ask for; the app's own folder needs none, and the
        // Downloads app still lists the file.
        request.setDestinationInExternalFilesDir(browser.activity, Environment.DIRECTORY_DOWNLOADS, name)
      }
      val queued = manager.enqueue(request)
      watching[queued] = Watch(id, tab.id, url, name, mime, tab.forAgents)
      report(id, tab, url, name, mime, "started", null, 0, -1)
      browser.ui.removeCallbacks(poll)
      browser.ui.postDelayed(poll, 500)
    } catch (e: Exception) {
      report(id, tab, url, name, mime, "failed", null, 0, -1)
    }
  }

  /** DownloadManager says nothing as it goes: it's asked, while anything is running. */
  private fun check() {
    if (watching.isEmpty()) return
    val ids = watching.keys.toLongArray()
    try {
      manager.query(DownloadManager.Query().setFilterById(*ids))?.use { rows ->
        val idCol = rows.getColumnIndexOrThrow(DownloadManager.COLUMN_ID)
        val statusCol = rows.getColumnIndexOrThrow(DownloadManager.COLUMN_STATUS)
        val doneCol = rows.getColumnIndexOrThrow(DownloadManager.COLUMN_BYTES_DOWNLOADED_SO_FAR)
        val totalCol = rows.getColumnIndexOrThrow(DownloadManager.COLUMN_TOTAL_SIZE_BYTES)
        val seen = mutableSetOf<Long>()
        while (rows.moveToNext()) {
          val queued = rows.getLong(idCol)
          val watch = watching[queued] ?: continue
          seen.add(queued)
          val received = rows.getLong(doneCol)
          val total = rows.getLong(totalCol)
          when (rows.getInt(statusCol)) {
            DownloadManager.STATUS_SUCCESSFUL -> {
              watching.remove(queued)
              val file = manager.getUriForDownloadedFile(queued)?.toString()
              report(watch, "done", file, received, total)
            }
            DownloadManager.STATUS_FAILED -> {
              watching.remove(queued)
              report(watch, "failed", null, received, total)
            }
            else -> if (received != watch.received) {
              watch.received = received
              report(watch, "started", null, received, total)
            }
          }
        }
        // Cancelled from the notification: gone from DownloadManager's list.
        for (queued in ids) {
          if (queued !in seen) watching.remove(queued)?.let { report(it, "cancelled", null, it.received, -1) }
        }
      }
    } catch (_: Exception) {
    }
    if (watching.isNotEmpty()) browser.ui.postDelayed(poll, 700)
  }

  private fun fromData(tab: Tab, url: String, name: String) {
    val id = ++next
    val comma = url.indexOf(',')
    val head = if (comma > 5) url.substring(5, comma) else ""
    val mime = head.split(';').firstOrNull()?.takeIf { it.contains('/') } ?: "text/plain"
    val file = withExtension(name, mime)
    report(id, tab, "data:", file, mime, "started", null, 0, -1)
    work.execute {
      val bytes = try {
        val body = url.substring(comma + 1)
        if (head.split(';').any { it.equals("base64", ignoreCase = true) }) Base64.decode(body, Base64.DEFAULT)
        else URLDecoder.decode(body.replace("+", "%2B"), "UTF-8").toByteArray()
      } catch (_: Exception) {
        null
      }
      val saved = bytes?.let { save(file, mime, it) }
      browser.ui.post {
        report(id, tab, "data:", file, mime, if (saved != null) "done" else "failed", saved?.toString(), bytes?.size?.toLong() ?: 0L, bytes?.size?.toLong() ?: -1L)
      }
    }
  }

  private fun fromBlob(tab: Tab, url: String, name: String, mime: String) {
    val id = ++next
    report(id, tab, url, name, mime, "started", null, 0, -1)
    browser.agent.readBlob(tab, url, MAX_FROM_PAGE) { bytes, type, _ ->
      if (bytes == null) {
        report(id, tab, url, name, mime, "failed", null, 0, -1)
        return@readBlob
      }
      val realType = type?.takeIf { it.isNotBlank() } ?: mime
      val file = withExtension(name, realType)
      work.execute {
        val saved = save(file, realType, bytes)
        browser.ui.post {
          report(id, tab, url, file, realType, if (saved != null) "done" else "failed", saved?.toString(), bytes.size.toLong(), bytes.size.toLong())
        }
      }
    }
  }

  /**
   * browser_save_file: the console's own bytes (a page's picture, say) into
   * Downloads. [data] is a data URL or plain base64; [done] hears where it
   * landed, or a problem.
   */
  fun saveFile(name: String, data: String, done: (String?, String?) -> Unit) {
    work.execute {
      val result: Pair<String?, String?> = try {
        val comma = data.indexOf(',')
        val (mime, body) = if (data.startsWith("data:") && comma > 0) {
          val head = data.substring(5, comma)
          (head.split(';').firstOrNull()?.takeIf { it.contains('/') } ?: guessType(name) ?: "application/octet-stream") to data.substring(comma + 1)
        } else {
          (guessType(name) ?: "application/octet-stream") to data
        }
        if (body.length > MAX_FROM_PAGE * 4 / 3) throw IllegalArgumentException("that file is too large to save from here")
        val bytes = Base64.decode(body, Base64.DEFAULT)
        val saved = save(cleanName(name.ifBlank { "download" }), mime, bytes)
          ?: throw IllegalStateException("the phone didn't let OpenRemote save to Downloads")
        Pair(saved.toString(), null)
      } catch (e: Exception) {
        Pair(null, e.message ?: "that file couldn't be saved")
      }
      browser.ui.post { done(result.first, result.second) }
    }
  }

  /** Writes bytes into Downloads; the file's address, or null. Runs off the main thread. */
  private fun save(name: String, mime: String, bytes: ByteArray): Uri? = try {
    val activity = browser.activity
    if (Build.VERSION.SDK_INT >= 29) {
      val resolver = activity.contentResolver
      val values = ContentValues().apply {
        put(MediaStore.MediaColumns.DISPLAY_NAME, name)
        put(MediaStore.MediaColumns.MIME_TYPE, mime)
        put(MediaStore.MediaColumns.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS)
        put(MediaStore.MediaColumns.IS_PENDING, 1)
      }
      val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
      if (uri != null) {
        resolver.openOutputStream(uri)?.use { it.write(bytes) }
        values.clear()
        values.put(MediaStore.MediaColumns.IS_PENDING, 0)
        resolver.update(uri, values, null, null)
      }
      uri
    } else {
      val dir = activity.getExternalFilesDir(Environment.DIRECTORY_DOWNLOADS) ?: activity.filesDir
      val file = unique(File(dir, name))
      file.writeBytes(bytes)
      @Suppress("DEPRECATION")
      manager.addCompletedDownload(file.name, file.name, true, mime, file.absolutePath, bytes.size.toLong(), true)
      FileProvider.getUriForFile(activity, "${activity.packageName}.fileprovider", file)
    }
  } catch (_: Exception) {
    null
  }

  private fun unique(file: File): File {
    if (!file.exists()) return file
    val base = file.nameWithoutExtension
    val ext = file.extension.let { if (it.isEmpty()) "" else ".$it" }
    var n = 1
    while (true) {
      val next = File(file.parentFile, "$base ($n)$ext")
      if (!next.exists()) return next
      n += 1
    }
  }

  private fun report(watch: Watch, state: String, path: String?, received: Long, total: Long) {
    emit(watch.id, watch.tab, watch.url, watch.name, watch.mime, state, path, received, total, watch.agents)
  }

  private fun report(id: Long, tab: Tab, url: String, name: String, mime: String, state: String, path: String?, received: Long, total: Long) {
    emit(id, tab.id, url, name, mime, state, path, received, total, tab.forAgents)
  }

  /** A download as the console lists it (the desktop's fields, and its size so far). */
  private fun emit(id: Long, tab: String, url: String, name: String, mime: String, state: String, path: String?, received: Long, total: Long, agents: Boolean) {
    browser.emit("download", JSObject().apply {
      put("id", id)
      put("tab", tab)
      put("url", if (url.length > 2000) url.take(2000) else url)
      put("name", name)
      put("mime", mime)
      put("state", state)
      put("path", path ?: JSONObject.NULL)
      put("received", received)
      put("total", total)
      put("agents", agents)
    })
  }

  /** browser_reveal: opens a finished download in whatever opens its kind. */
  fun reveal(path: String): String? {
    val activity = browser.activity
    val uri = if (path.startsWith("/")) {
      try {
        FileProvider.getUriForFile(activity, "${activity.packageName}.fileprovider", File(path))
      } catch (_: Exception) {
        return "that file is gone"
      }
    } else {
      Uri.parse(path)
    }
    val type = activity.contentResolver.getType(uri) ?: guessType(path) ?: "*/*"
    val view = Intent(Intent.ACTION_VIEW)
      .setDataAndType(uri, type)
      .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
    return try {
      activity.startActivity(view)
      null
    } catch (_: ActivityNotFoundException) {
      // Nothing opens that kind of file: the Downloads list at least shows it.
      try {
        activity.startActivity(Intent(DownloadManager.ACTION_VIEW_DOWNLOADS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        null
      } catch (_: Exception) {
        "nothing on this phone opens that file"
      }
    } catch (e: Exception) {
      e.message ?: "that file couldn't be opened"
    }
  }

  /** A page's file picker: the phone's own, for one file or several. */
  fun choose(callback: ValueCallback<Array<Uri>>, params: WebChromeClient.FileChooserParams): Boolean {
    val intent = try {
      params.createIntent()
    } catch (_: Exception) {
      callback.onReceiveValue(null)
      return true
    }
    if (params.mode == WebChromeClient.FileChooserParams.MODE_OPEN_MULTIPLE) {
      intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
    }
    try {
      PluginManager.startActivityForResult(intent) { result ->
        val data = result.data
        val clip = data?.clipData
        val picked = if (clip != null && clip.itemCount > 0) {
          Array(clip.itemCount) { clip.getItemAt(it).uri }
        } else {
          WebChromeClient.FileChooserParams.parseResult(result.resultCode, data)
        }
        callback.onReceiveValue(picked)
      }
    } catch (_: Exception) {
      callback.onReceiveValue(null)
    }
    return true
  }

  /**
   * A link to an app (mail, the phone, a map, a store, an intent) the person
   * tapped: the app opens, as from Chrome. An intent's own fallback page
   * opens in the tab when no app takes it.
   */
  fun openApp(tab: Tab, uri: Uri) {
    val activity = browser.activity
    try {
      if (uri.scheme.equals("intent", ignoreCase = true)) {
        val intent = Intent.parseUri(uri.toString(), Intent.URI_INTENT_SCHEME)
        // A page never names a component directly: only what any browser may open.
        intent.addCategory(Intent.CATEGORY_BROWSABLE)
        intent.component = null
        intent.selector = null
        try {
          activity.startActivity(intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        } catch (_: ActivityNotFoundException) {
          val fallback = intent.getStringExtra("browser_fallback_url")
          if (fallback != null && browser.refused(fallback) == null) browser.navigate(tab, fallback)
        }
        return
      }
      activity.startActivity(
        Intent(Intent.ACTION_VIEW, uri)
          .addCategory(Intent.CATEGORY_BROWSABLE)
          .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
      )
    } catch (_: Exception) {
      // No app for it: the page stays where it is.
    }
  }

  /** browser_open_external: the page in the phone's own browser. */
  fun openOutside(url: String): String? = try {
    browser.activity.startActivity(
      Intent(Intent.ACTION_VIEW, Uri.parse(url))
        .addCategory(Intent.CATEGORY_BROWSABLE)
        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
    )
    null
  } catch (_: Exception) {
    "no other browser on this phone opens it"
  }

  /** browser_share: the phone's share sheet, with the page's address. */
  fun share(url: String, title: String?): String? = try {
    val send = Intent(Intent.ACTION_SEND)
      .setType("text/plain")
      .putExtra(Intent.EXTRA_TEXT, url)
    if (!title.isNullOrBlank()) send.putExtra(Intent.EXTRA_SUBJECT, title)
    browser.activity.startActivity(Intent.createChooser(send, null).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    null
  } catch (e: Exception) {
    e.message ?: "the share sheet didn't open"
  }

  private fun guessType(name: String): String? {
    val ext = MimeTypeMap.getFileExtensionFromUrl(name).takeIf { it.isNotEmpty() }
      ?: name.substringAfterLast('.', "").lowercase().takeIf { it.isNotEmpty() && it.length <= 8 }
      ?: return null
    return MimeTypeMap.getSingleton().getMimeTypeFromExtension(ext.lowercase())
  }

  private fun withExtension(name: String, mime: String): String {
    if (name.contains('.') && !name.endsWith(".bin")) return name
    val ext = MimeTypeMap.getSingleton().getExtensionFromMimeType(mime) ?: return name
    return name.substringBeforeLast(".bin") + "." + ext
  }

  /** A name the phone's file systems accept. */
  private fun cleanName(name: String): String {
    val clean = name.replace(Regex("""[\\/:*?"<>|\u0000-\u001f]"""), "_").trim().trimStart('.')
    return clean.ifEmpty { "download" }.take(120)
  }
}
