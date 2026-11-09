package app.blackhole.browser

import android.annotation.SuppressLint
import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.net.Uri
import android.net.http.SslError
import android.os.Handler
import android.os.Looper
import android.util.Base64
import android.view.View
import android.webkit.GeolocationPermissions
import android.webkit.PermissionRequest
import android.webkit.SslErrorHandler
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.webkit.WebSettingsCompat
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import org.json.JSONArray
import org.json.JSONObject
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream

/**
 * The WebView of one tab. Pages are untrusted: they only reach the core through the
 * `bhPage` listener (`PageEvent`s, main frame only), and every main-frame navigation they
 * start is checked by the core first (Shields, https upgrade, external schemes).
 */
@SuppressLint("SetJavaScriptEnabled", "ViewConstructor")
class TabView(
    context: Context,
    val tab: Long,
    val profile: String,
    script: String,
    private val host: Host,
) : WebView(context) {

    interface Host {
        /** Sends a `NativeEvent`; returns its effects (the shell runs the asynchronous ones). */
        fun event(json: JSONObject): JSONArray
        fun pageEvent(tab: Long, json: String)
        fun progress(tab: Long, value: Int)
        fun navigated(tab: Long)
        /** Long press on a link ([url]), an image ([image]) or a linked image (both). */
        fun linkMenu(tab: Long, url: String?, image: String?)
        /** A video (or any element) goes full screen; [exitFullscreen] when the page leaves it. */
        fun fullscreen(view: View, callback: WebChromeClient.CustomViewCallback)
        fun exitFullscreen()
        /** `<input type=file>`: `false` when no app can pick a file (the page gets nothing). */
        fun chooseFiles(callback: ValueCallback<Array<Uri>>, params: WebChromeClient.FileChooserParams): Boolean
        /** A site asks for `camera`, `microphone`, `location`…: [answer] is called once. */
        fun askPermission(tab: Long, origin: String, kinds: List<String>, answer: (Boolean) -> Unit)
        /** Find in page results of this tab. */
        fun found(tab: Long, active: Int, count: Int)
        /** A request of [tab] was refused (any thread): the shield's count changed. */
        fun blocked(tab: Long)
        fun download(url: String, userAgent: String, disposition: String, mime: String, profile: String)
        fun isOnline(): Boolean
    }

    /** The page address, readable from network threads (`shouldInterceptRequest`). */
    @Volatile
    var pageUrl: String = ""
        private set

    private val debug = context.applicationInfo.flags and android.content.pm.ApplicationInfo.FLAG_DEBUGGABLE != 0

    /** The page's icon, for "Add to home screen". */
    var pageIcon: Bitmap? = null
        private set

    /**
     * The system's user agent without the WebView tokens (`; wv`, `Version/4.0`): sites treat it
     * as Chrome. Lazy: read after the profile is set (the first thing done to the view).
     */
    private val mobileAgent: String by lazy {
        WebSettings.getDefaultUserAgent(context).replace("; wv)", ")").replace(Regex("""Version/[\d.]+ """), "")
    }

    /** `requestFocusNodeHref` answers here: the link and the image of a linked image. */
    private val hrefs = Handler(Looper.getMainLooper()) { msg ->
        host.linkMenu(tab, msg.data.getString("url"), msg.data.getString("src"))
        true
    }
    init {
        // A profile is chosen before anything else touches the view.
        if (WebViewFeature.isFeatureSupported(WebViewFeature.MULTI_PROFILE)) {
            WebViewCompat.setProfile(this, profile)
        }
        settings.apply {
            javaScriptEnabled = true
            domStorageEnabled = true
            allowFileAccess = false
            allowContentAccess = false
            mixedContentMode = WebSettings.MIXED_CONTENT_NEVER_ALLOW
            setSupportMultipleWindows(false)
            setSupportZoom(true)
            builtInZoomControls = true
            displayZoomControls = false
            // Pages without a viewport tag are laid out like a desktop page, then fitted.
            useWideViewPort = true
            loadWithOverviewMode = true
            setGeolocationEnabled(true)
            userAgentString = mobileAgent
            safeBrowsingEnabled = true
            mediaPlaybackRequiresUserGesture = true
        }
        if (WebViewFeature.isFeatureSupported(WebViewFeature.ALGORITHMIC_DARKENING)) {
            // Sites keep their own colours; our pages follow the theme themselves.
            WebSettingsCompat.setAlgorithmicDarkeningAllowed(settings, false)
        }
        if (WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)) {
            WebViewCompat.addDocumentStartJavaScript(this, script, setOf("*"))
        }
        if (WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) {
            WebViewCompat.addWebMessageListener(this, "bhPage", setOf("*")) { _, message, _, isMainFrame, _ ->
                val data = message.data
                if (isMainFrame && data != null && data.length < 4096) host.pageEvent(tab, data)
            }
        }
        setBackgroundColor(0xFF0C0B12.toInt())
        webViewClient = Client()
        webChromeClient = Chrome()
        setDownloadListener { url, userAgent, disposition, mime, _ ->
            host.download(url, userAgent, disposition ?: "", mime ?: "", profile)
        }
        setOnLongClickListener {
            val hit = hitTestResult
            when (hit.type) {
                HitTestResult.SRC_ANCHOR_TYPE -> {
                    host.linkMenu(tab, hit.extra, null)
                    true
                }
                // `extra` is the image here: the link comes from `requestFocusNodeHref`.
                HitTestResult.SRC_IMAGE_ANCHOR_TYPE -> {
                    requestFocusNodeHref(hrefs.obtainMessage())
                    true
                }
                HitTestResult.IMAGE_TYPE -> {
                    host.linkMenu(tab, null, hit.extra)
                    true
                }
                else -> false
            }
        }
        setFindListener { active, count, done -> if (done) host.found(tab, if (count == 0) 0 else active + 1, count) }
    }

    /** Desktop site: a desktop user agent (the page is reloaded when [reload]). */
    fun setDesktop(on: Boolean, reload: Boolean) {
        val agent = if (on) desktopAgent(mobileAgent) else mobileAgent
        if (settings.userAgentString == agent) return
        settings.userAgentString = agent
        if (reload) reload()
    }

    private fun desktopAgent(mobile: String): String =
        mobile.replace(Regex("""\(Linux; Android[^)]*\)"""), "(X11; Linux x86_64)").replace(" Mobile", "")

    /** The icon as a small PNG `data:` URL (64 px at most). */
    private fun iconData(icon: Bitmap): String? = try {
        val side = 64
        val scaled = if (icon.width > side || icon.height > side) {
            val k = side.toFloat() / maxOf(icon.width, icon.height)
            Bitmap.createScaledBitmap(icon, (icon.width * k).toInt().coerceAtLeast(1), (icon.height * k).toInt().coerceAtLeast(1), true)
        } else {
            icon
        }
        val out = ByteArrayOutputStream()
        scaled.compress(Bitmap.CompressFormat.PNG, 100, out)
        "data:image/png;base64," + Base64.encodeToString(out.toByteArray(), Base64.NO_WRAP)
    } catch (e: RuntimeException) {
        null
    }

    /** A small JPEG of the page for the tab grid, as a `data:` URL. */
    fun thumbnail(): String? {
        if (width <= 0 || height <= 0) return null
        val scale = 0.33f
        val w = (width * scale).toInt().coerceAtLeast(1)
        val h = (height * scale).toInt().coerceAtLeast(1)
        return try {
            val bitmap = Bitmap.createBitmap(w, h, Bitmap.Config.RGB_565)
            val canvas = Canvas(bitmap)
            canvas.scale(scale, scale)
            canvas.translate(-scrollX.toFloat(), -scrollY.toFloat())
            draw(canvas)
            val out = ByteArrayOutputStream()
            bitmap.compress(Bitmap.CompressFormat.JPEG, 55, out)
            bitmap.recycle()
            "data:image/jpeg;base64," + Base64.encodeToString(out.toByteArray(), Base64.NO_WRAP)
        } catch (e: RuntimeException) {
            null
        }
    }

    private fun ev(name: String, vararg pairs: Pair<String, Any>): JSONArray {
        val json = JSONObject().put("ev", name).put("tab", tab)
        for ((k, v) in pairs) json.put(k, v)
        return host.event(json)
    }

    private fun has(effects: JSONArray, what: String): Boolean =
        (0 until effects.length()).any { effects.optJSONObject(it)?.optString("do") == what }

    private inner class Client : WebViewClient() {
        override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
            if (!request.isForMainFrame) return false
            return has(ev("navigating", "url" to request.url.toString()), "cancel")
        }

        override fun onPageStarted(view: WebView, url: String, icon: Bitmap?) {
            pageUrl = url
            pageIcon = null
            ev("started", "url" to url)
            host.navigated(tab)
        }

        override fun onPageFinished(view: WebView, url: String) {
            ev("finished", "url" to url, "title" to (view.title ?: ""))
            host.navigated(tab)
        }

        override fun doUpdateVisitedHistory(view: WebView, url: String, isReload: Boolean) {
            host.navigated(tab)
        }

        override fun onReceivedError(view: WebView, request: WebResourceRequest, error: WebResourceError) {
            if (!request.isForMainFrame) return
            ev("failed", "url" to request.url.toString(), "code" to error.errorCode, "online" to host.isOnline())
        }

        @SuppressLint("WebViewClientOnReceivedSslError")
        override fun onReceivedSslError(view: WebView, handler: SslErrorHandler, error: SslError) {
            // Proceeds only for a host the user accepted through the shell's own menu.
            val effects = ev("ssl_error", "url" to error.url, "primary" to error.primaryError)
            val proceed = (0 until effects.length()).any {
                val e = effects.optJSONObject(it)
                e?.optString("do") == "ssl" && e.optBoolean("proceed")
            }
            if (proceed) handler.proceed() else handler.cancel()
        }

        override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? {
            val url = request.url.toString()
            if (request.isForMainFrame) {
                // Set here, on the network thread: `onPageStarted` reaches the UI thread after the
                // page's first subresource requests, which would be counted for the previous page.
                pageUrl = url
                return null
            }
            if (!url.startsWith("http")) return null
            val blocked = Native.shouldBlock(url, pageUrl)
            if (debug) android.util.Log.d("BlackHole", "request ${if (blocked) "blocked" else "allowed"}: $url")
            if (blocked) host.blocked(tab)
            return if (blocked) {
                WebResourceResponse("text/plain", "utf-8", 403, "Blocked", emptyMap(), ByteArrayInputStream(ByteArray(0)))
            } else {
                null
            }
        }
    }

    private inner class Chrome : WebChromeClient() {
        override fun onProgressChanged(view: WebView, newProgress: Int) {
            host.progress(tab, newProgress)
        }

        override fun onReceivedTitle(view: WebView, title: String?) {
            if (!title.isNullOrBlank()) ev("title", "title" to title)
        }

        override fun onReceivedIcon(view: WebView, icon: Bitmap?) {
            icon ?: return
            pageIcon = icon
            iconData(icon)?.let { ev("favicon", "data" to it) }
        }

        override fun onShowCustomView(view: View, callback: WebChromeClient.CustomViewCallback) = host.fullscreen(view, callback)

        override fun onHideCustomView() = host.exitFullscreen()

        override fun onShowFileChooser(view: WebView, callback: ValueCallback<Array<Uri>>, params: WebChromeClient.FileChooserParams): Boolean =
            host.chooseFiles(callback, params)

        override fun onPermissionRequest(request: PermissionRequest) {
            val kinds = request.resources.map {
                when (it) {
                    PermissionRequest.RESOURCE_VIDEO_CAPTURE -> "camera"
                    PermissionRequest.RESOURCE_AUDIO_CAPTURE -> "microphone"
                    PermissionRequest.RESOURCE_PROTECTED_MEDIA_ID -> "protected_media"
                    else -> "other" // refused by the core
                }
            }
            host.askPermission(tab, request.origin.toString(), kinds) { ok ->
                if (ok) request.grant(request.resources) else request.deny()
            }
        }

        override fun onGeolocationPermissionsShowPrompt(origin: String, callback: GeolocationPermissions.Callback) {
            host.askPermission(tab, origin, listOf("location")) { ok -> callback.invoke(origin, ok, false) }
        }
    }
}
