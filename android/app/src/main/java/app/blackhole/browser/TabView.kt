package app.blackhole.browser

import android.annotation.SuppressLint
import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.net.http.SslError
import android.util.Base64
import android.webkit.SslErrorHandler
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
        fun linkMenu(tab: Long, url: String)
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
            val url = hit.extra
            if (url != null && (hit.type == HitTestResult.SRC_ANCHOR_TYPE || hit.type == HitTestResult.SRC_IMAGE_ANCHOR_TYPE)) {
                host.linkMenu(tab, url)
                true
            } else {
                false
            }
        }
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

        override fun onPageStarted(view: WebView, url: String, favicon: Bitmap?) {
            pageUrl = url
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
    }
}
