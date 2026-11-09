package app.blackhole.browser

import android.annotation.SuppressLint
import android.app.DownloadManager
import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.graphics.Color
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.Uri
import android.os.Bundle
import android.os.Environment
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.animation.DecelerateInterpolator
import android.webkit.CookieManager
import android.webkit.URLUtil
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebStorage
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.FrameLayout
import androidx.activity.ComponentActivity
import androidx.activity.OnBackPressedCallback
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.webkit.JavaScriptReplyProxy
import androidx.webkit.ProfileStore
import androidx.webkit.WebViewAssetLoader
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

/**
 * The Kotlin shell: tab WebViews under one transparent interface WebView (`mobile.html`).
 * The interface only takes the touches of the areas it declares (the bottom bar, or the whole
 * screen while the start page, drawer, tab grid or a sheet is open); the rest reaches the tab.
 */
class MainActivity : ComponentActivity(), TabView.Host {

    private val main = Handler(Looper.getMainLooper())
    private lateinit var root: FrameLayout
    private lateinit var tabsLayer: FrameLayout
    private lateinit var shell: ShellView
    private val tabs = HashMap<Long, TabView>()
    private var current: Long? = null

    private var ui: JavaScriptReplyProxy? = null
    private val pendingUi = ArrayList<String>()

    // What the interface declared (CSS px are converted with the density).
    private var surfaceFull = true
    private var overlay = false
    private var barPx = 0
    private var animations = true
    private var slide = 0
    private var insets = JSONObject()

    private val downloads = ArrayList<Long>()
    private val density get() = resources.displayMetrics.density
    private val multiProfile by lazy { WebViewFeature.isFeatureSupported(WebViewFeature.MULTI_PROFILE) }
    private val debuggable get() = applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0

    companion object {
        private const val ORIGIN = "https://appassets.androidplatform.net"
        private const val SHELL_URL = "$ORIGIN/assets/mobile.html"
        /** Interface messages the shell handles itself; every other one goes to the core. */
        private val LOCAL = setOf("surface", "thumbs", "peek", "peek_end", "copy", "share", "open_downloads")
    }

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        WindowCompat.setDecorFitsSystemWindows(window, false)
        if (debuggable) WebView.setWebContentsDebuggingEnabled(true)

        root = FrameLayout(this)
        root.setBackgroundColor(0xFF0C0B12.toInt())
        tabsLayer = FrameLayout(this)
        root.addView(tabsLayer, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        shell = ShellView(this)
        root.addView(shell, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        setContentView(root)

        ViewCompat.setOnApplyWindowInsetsListener(root) { _, all ->
            val bars = all.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
            val ime = all.getInsets(WindowInsetsCompat.Type.ime())
            insets = JSONObject()
                .put("type", "insets")
                .put("top", bars.top / density)
                .put("bottom", maxOf(bars.bottom, ime.bottom) / density)
                .put("left", bars.left / density)
                .put("right", bars.right / density)
                .put("keyboard", ime.bottom > 0)
            (tabsLayer.layoutParams as FrameLayout.LayoutParams).topMargin = bars.top
            tabsLayer.requestLayout()
            postUi(insets.toString())
            all
        }

        setUpShell()
        // Debug builds only: `--es bh_peer ip:port` points the devices service at one address
        // (the emulator reaching BlackHole on its host, see scripts/test-android.ps1).
        val peer = if (debuggable) intent?.getStringExtra("bh_peer").orEmpty() else ""
        apply(Native.init(File(filesDir, "profile").absolutePath, KeyVault.devicesKeyHex(this), deviceName(), peer))
        handleIntent(intent)

        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() = back()
        })
    }

    @SuppressLint("SetJavaScriptEnabled")
    private fun setUpShell() {
        val loader = WebViewAssetLoader.Builder()
            .addPathHandler("/assets/", WebViewAssetLoader.AssetsPathHandler(this))
            .build()
        shell.setBackgroundColor(Color.TRANSPARENT)
        shell.settings.apply {
            javaScriptEnabled = true
            domStorageEnabled = true
            allowFileAccess = false
            allowContentAccess = false
        }
        shell.webViewClient = object : WebViewClient() {
            override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? =
                loader.shouldInterceptRequest(request.url)

            // The interface never navigates anywhere.
            override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest) = true
        }
        if (debuggable) {
            // The test script reads `bheval:` lines of the interface's console from logcat.
            shell.webChromeClient = object : android.webkit.WebChromeClient() {
                override fun onConsoleMessage(message: android.webkit.ConsoleMessage): Boolean {
                    android.util.Log.i("BlackHole", message.message())
                    return true
                }
            }
        }
        // Only our own page (the asset origin, main frame) can talk to the shell.
        WebViewCompat.addWebMessageListener(shell, "bh", setOf(ORIGIN)) { _, message, origin, isMainFrame, proxy ->
            if (!isMainFrame || origin.toString() != ORIGIN) return@addWebMessageListener
            val data = message.data ?: return@addWebMessageListener
            if (ui == null) {
                ui = proxy
                postUi(envMessage())
                postUi(insets.toString())
                pendingUi.forEach { proxy.postMessage(it) }
                pendingUi.clear()
            }
            fromUi(data)
        }
        shell.loadUrl(SHELL_URL)
    }

    private fun envMessage(): String {
        val scale = Settings.Global.getFloat(contentResolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f)
        return JSONObject()
            .put("type", "env")
            .put("reduced", scale == 0f)
            .put("profiles", multiProfile)
            .put("debug", debuggable)
            .toString()
    }

    private fun postUi(json: String) {
        val proxy = ui
        if (proxy == null) {
            if (pendingUi.size < 64) pendingUi.add(json)
        } else {
            proxy.postMessage(json)
        }
    }

    private fun fromUi(data: String) {
        val msg = try {
            JSONObject(data)
        } catch (e: org.json.JSONException) {
            return
        }
        when (val cmd = msg.optString("cmd")) {
            in LOCAL -> local(cmd, msg)
            else -> apply(Native.command(data))
        }
    }

    private fun local(cmd: String, msg: JSONObject) {
        when (cmd) {
            "surface" -> {
                surfaceFull = msg.optBoolean("full", true)
                overlay = msg.optBoolean("overlay", false)
                animations = msg.optBoolean("animations", true)
                barPx = (msg.optDouble("bottom", 0.0) * density).toInt()
                (tabsLayer.layoutParams as FrameLayout.LayoutParams).bottomMargin = barPx
                tabsLayer.requestLayout()
                val dark = msg.optBoolean("dark", true)
                WindowCompat.getInsetsController(window, root).apply {
                    isAppearanceLightStatusBars = !dark
                    isAppearanceLightNavigationBars = !dark
                }
                tabsLayer.visibility = if (msg.optBoolean("cover", false)) View.INVISIBLE else View.VISIBLE
            }
            "thumbs" -> tabs.values.filter { it.isShown }.forEach { postThumb(it) }
            "peek" -> current?.let { tabs[it] }?.let { v ->
                val dx = msg.optDouble("dx", 0.0).toFloat() * density
                v.translationX = dx
                v.alpha = 1f - minOf(0.5f, kotlin.math.abs(dx) / v.width.coerceAtLeast(1))
            }
            "peek_end" -> {
                slide = msg.optInt("dir", 0)
                if (slide == 0) current?.let { tabs[it] }?.animate()?.translationX(0f)?.alpha(1f)?.setDuration(180)?.start()
            }
            "copy" -> {
                val text = msg.optString("text").take(4096)
                val clip = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                clip.setPrimaryClip(ClipData.newPlainText("link", text))
            }
            "share" -> {
                val url = msg.optString("url")
                if (URLUtil.isNetworkUrl(url)) {
                    val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, url)
                    startActivity(Intent.createChooser(send, null))
                }
            }
            "open_downloads" -> try {
                startActivity(Intent(DownloadManager.ACTION_VIEW_DOWNLOADS))
            } catch (e: ActivityNotFoundException) {
                toast("No app shows downloads")
            }
        }
    }

    private fun toast(text: String) =
        postUi(JSONObject().put("type", "toast").put("text", text).toString())

    /** Runs the core's effects in order. */
    private fun apply(json: String) {
        val effects = try {
            JSONArray(json)
        } catch (e: org.json.JSONException) {
            return
        }
        for (i in 0 until effects.length()) effects.optJSONObject(i)?.let { run(it) }
    }

    private fun run(e: JSONObject) {
        val tab = e.optLong("tab", -1)
        when (e.optString("do")) {
            "open" -> {
                val view = tabs[tab] ?: TabView(this, tab, e.optString("profile"), e.optString("script"), this).also {
                    it.visibility = View.GONE
                    tabsLayer.addView(it, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
                    tabs[tab] = it
                }
                view.loadUrl(e.optString("url"))
            }
            "show" -> show(if (e.isNull("tab")) null else tab)
            "load" -> tabs[tab]?.loadUrl(e.optString("url"))
            "html" -> tabs[tab]?.loadDataWithBaseURL(null, e.optString("html"), "text/html", "utf-8", e.optString("url"))
            "nav" -> tabs[tab]?.let { v ->
                when (e.optString("action")) {
                    "back" -> if (v.canGoBack()) v.goBack()
                    "forward" -> if (v.canGoForward()) v.goForward()
                    "reload" -> v.reload()
                }
            }
            "drop" -> tabs.remove(tab)?.let { v ->
                tabsLayer.removeView(v)
                v.destroy()
                postUi(JSONObject().put("type", "dropped").put("tab", tab).toString())
            }
            "ui" -> e.optJSONObject("msg")?.let { postUi(it.toString()) }
            "clear_profile" -> clearProfile(e.optString("profile"))
            "external" -> external(e.optString("url"))
            "exit" -> moveTaskToBack(true)
            // "ssl" and "cancel" answer a WebView callback synchronously (TabView); "download"
            // answers the DownloadListener (download()), which has the request's user agent.
        }
    }

    private fun show(tab: Long?) {
        val before = current?.let { tabs[it] }
        current = tab
        val next = tab?.let { tabs[it] }
        if (before != null && before != next) postThumb(before)
        val dir = slide
        slide = 0
        val width = tabsLayer.width.toFloat()
        for ((id, v) in tabs) {
            if (id == tab || (v == before && dir != 0 && animations && next != null)) continue
            v.visibility = View.GONE
            v.translationX = 0f
            v.alpha = 1f
        }
        if (next == null) {
            before?.visibility = View.GONE
            return
        }
        next.visibility = View.VISIBLE
        if (dir != 0 && animations && before != null && before != next) {
            // The swipe on the bar: the old page leaves on one side, the new one comes in.
            next.translationX = dir * width
            next.alpha = 1f
            next.animate().translationX(0f).setDuration(240).setInterpolator(DecelerateInterpolator(2f)).start()
            before.animate().translationX(-dir * width).setDuration(240).setInterpolator(DecelerateInterpolator(2f))
                .withEndAction {
                    if (current != before.tab) before.visibility = View.GONE
                    before.translationX = 0f
                    before.alpha = 1f
                }.start()
        } else {
            next.translationX = 0f
            next.alpha = 1f
        }
        navigated(next.tab)
    }

    private fun postThumb(view: TabView) {
        val data = view.thumbnail() ?: return
        postUi(JSONObject().put("type", "thumb").put("tab", view.tab).put("data", data).toString())
    }

    private fun back() {
        val view = current?.let { tabs[it] }
        when {
            overlay -> postUi("""{"type":"back"}""")
            view != null && view.canGoBack() -> view.goBack()
            else -> apply(Native.event("""{"ev":"back_exhausted"}"""))
        }
    }

    private fun clearProfile(name: String) {
        if (multiProfile) {
            val store = ProfileStore.getInstance()
            val names = if (name == "*") store.allProfileNames else listOf(name)
            for (n in names) {
                val profile = store.getProfile(n) ?: continue
                profile.cookieManager.removeAllCookies(null)
                profile.webStorage.deleteAllData()
            }
            // A private profile nobody uses any more is deleted altogether.
            if (name == "private" && tabs.values.none { it.profile == "private" }) {
                main.post {
                    try {
                        store.deleteProfile("private")
                    } catch (e: IllegalStateException) {
                        // Still in use by a view being destroyed: its data is already cleared.
                    }
                }
            }
        } else if (name == "*") {
            CookieManager.getInstance().removeAllCookies(null)
            WebStorage.getInstance().deleteAllData()
        }
        if (name == "*") tabs.values.firstOrNull()?.clearCache(true)
    }

    private fun external(url: String) {
        try {
            val intent = if (url.startsWith("intent:")) {
                // Only browsable activities, never a component the page names.
                Intent.parseUri(url, Intent.URI_INTENT_SCHEME).apply {
                    addCategory(Intent.CATEGORY_BROWSABLE)
                    component = null
                    selector = null
                }
            } else {
                Intent(Intent.ACTION_VIEW, Uri.parse(url)).addCategory(Intent.CATEGORY_BROWSABLE)
            }
            startActivity(intent)
        } catch (e: ActivityNotFoundException) {
            toast("No app can open this link")
        } catch (e: java.net.URISyntaxException) {
            toast("This link is malformed")
        }
    }

    // TabView.Host

    override fun event(json: JSONObject): JSONArray {
        val effects = try {
            JSONArray(Native.event(json.toString()))
        } catch (e: org.json.JSONException) {
            JSONArray()
        }
        // Views may be created or destroyed: never from inside a WebView callback.
        val later = JSONArray()
        for (i in 0 until effects.length()) {
            val e = effects.optJSONObject(i) ?: continue
            if (e.optString("do") !in setOf("ssl", "cancel")) later.put(e)
        }
        if (later.length() > 0) main.post { apply(later.toString()) }
        return effects
    }

    override fun pageEvent(tab: Long, json: String) {
        main.post { apply(Native.pageEvent(tab, json)) }
    }

    override fun progress(tab: Long, value: Int) {
        if (tab == current) postUi(JSONObject().put("type", "progress").put("tab", tab).put("value", value).toString())
    }

    override fun navigated(tab: Long) {
        val v = tabs[tab] ?: return
        if (tab != current) return
        postUi(JSONObject().put("type", "nav").put("tab", tab).put("back", v.canGoBack()).put("forward", v.canGoForward()).toString())
    }

    private val refreshState = Runnable { apply(Native.command("""{"cmd":"ready"}""")) }

    /** What the paired devices sent, a few times a second while the app is on screen. */
    private val tick = object : Runnable {
        override fun run() {
            apply(Native.event("""{"ev":"tick"}"""))
            main.postDelayed(this, 300)
        }
    }

    /** Broadcasts on the Wi-Fi (devices finding each other) are filtered without this lock. */
    private val multicast by lazy {
        (applicationContext.getSystemService(Context.WIFI_SERVICE) as android.net.wifi.WifiManager)
            .createMulticastLock("blackhole-devices").apply { setReferenceCounted(false) }
    }

    /** The name other devices show ("Continue on …"): the one set in the system, else the model. */
    private fun deviceName(): String =
        Settings.Global.getString(contentResolver, Settings.Global.DEVICE_NAME)?.takeIf { it.isNotBlank() }
            ?: android.os.Build.MODEL ?: "Android"

    override fun blocked(tab: Long) {
        // At most one state refresh every half second, however many requests are refused.
        main.post {
            if (tab == current && !main.hasCallbacks(refreshState)) main.postDelayed(refreshState, 500)
        }
    }

    override fun linkMenu(tab: Long, url: String) {
        if (URLUtil.isNetworkUrl(url)) postUi(JSONObject().put("type", "link_menu").put("url", url).toString())
    }

    override fun download(url: String, userAgent: String, disposition: String, mime: String, profile: String) {
        val effects = Native.event(JSONObject().put("ev", "download").put("url", url).put("mime", mime).put("disposition", disposition).toString())
        // The core answers "download" only for an allowed (https) address.
        val e = JSONArray(effects)
        for (i in 0 until e.length()) {
            val x = e.optJSONObject(i) ?: continue
            if (x.optString("do") == "download") enqueue(url, userAgent, disposition, mime, profile) else run(x)
        }
    }

    private fun enqueue(url: String, userAgent: String, disposition: String, mime: String, profile: String) {
        val name = URLUtil.guessFileName(url, disposition, mime)
        val cookies = if (multiProfile) ProfileStore.getInstance().getProfile(profile)?.cookieManager?.getCookie(url)
        else CookieManager.getInstance().getCookie(url)
        val request = DownloadManager.Request(Uri.parse(url))
            .setMimeType(mime.ifEmpty { null })
            .setTitle(name)
            .addRequestHeader("User-Agent", userAgent)
            .setNotificationVisibility(DownloadManager.Request.VISIBILITY_VISIBLE_NOTIFY_COMPLETED)
            .setDestinationInExternalPublicDir(Environment.DIRECTORY_DOWNLOADS, "BlackHole/$name")
        // Private tabs never hand their cookies to the system download service.
        if (profile != "private" && cookies != null) request.addRequestHeader("Cookie", cookies)
        val dm = getSystemService(Context.DOWNLOAD_SERVICE) as DownloadManager
        downloads.add(dm.enqueue(request))
        pollDownloads()
    }

    private fun pollDownloads() {
        val dm = getSystemService(Context.DOWNLOAD_SERVICE) as DownloadManager
        if (downloads.isEmpty()) return
        val items = JSONArray()
        var running = false
        dm.query(DownloadManager.Query().setFilterById(*downloads.toLongArray()))?.use { c ->
            while (c.moveToNext()) {
                val status = c.getInt(c.getColumnIndexOrThrow(DownloadManager.COLUMN_STATUS))
                val active = status == DownloadManager.STATUS_RUNNING || status == DownloadManager.STATUS_PENDING || status == DownloadManager.STATUS_PAUSED
                running = running || active
                items.put(
                    JSONObject()
                        .put("name", c.getString(c.getColumnIndexOrThrow(DownloadManager.COLUMN_TITLE)))
                        .put("done", c.getLong(c.getColumnIndexOrThrow(DownloadManager.COLUMN_BYTES_DOWNLOADED_SO_FAR)))
                        .put("total", c.getLong(c.getColumnIndexOrThrow(DownloadManager.COLUMN_TOTAL_SIZE_BYTES)))
                        .put("status", when (status) {
                            DownloadManager.STATUS_SUCCESSFUL -> "done"
                            DownloadManager.STATUS_FAILED -> "failed"
                            else -> "running"
                        }),
                )
            }
        }
        postUi(JSONObject().put("type", "downloads").put("items", items).toString())
        if (running) main.postDelayed({ pollDownloads() }, 1000)
    }

    override fun isOnline(): Boolean {
        val cm = getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
        val caps = cm.getNetworkCapabilities(cm.activeNetwork) ?: return false
        return caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
    }

    // Lifecycle

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleIntent(intent)
    }

    private fun handleIntent(intent: Intent?) {
        intent ?: return
        val data = intent.dataString
        if (intent.action == Intent.ACTION_VIEW && data != null && URLUtil.isNetworkUrl(data)) {
            apply(Native.event(JSONObject().put("ev", "intent").put("url", data).toString()))
        }
        // Debug builds only: the test script (scripts/test-android.ps1) drives the app with
        // `adb shell am start ... --es bh_cmd64 <base64 JSON>` (an interface message) and
        // `--es bh_eval64 <base64 JS>` (run in the interface page; results go to logcat).
        if (debuggable) {
            fun decode(key: String) = intent.getStringExtra(key)?.let { String(android.util.Base64.decode(it, android.util.Base64.DEFAULT)) }
            decode("bh_cmd64")?.let { main.postDelayed({ fromUi(it) }, 50) }
            decode("bh_eval64")?.let { js -> main.postDelayed({ shell.evaluateJavascript(js, null) }, 50) }
        }
    }

    override fun onPause() {
        super.onPause()
        main.removeCallbacks(tick)
        if (multicast.isHeld) multicast.release()
        current?.let { tabs[it] }?.onPause()
        apply(Native.event("""{"ev":"pause"}"""))
    }

    override fun onResume() {
        super.onResume()
        multicast.acquire()
        main.removeCallbacks(tick)
        main.post(tick)
        current?.let { tabs[it] }?.onResume()
        postUi(envMessage())
    }

    override fun onDestroy() {
        tabs.values.forEach { it.destroy() }
        tabs.clear()
        shell.destroy()
        super.onDestroy()
    }

    /** The interface view: takes only the touches of the areas the page declared. */
    private inner class ShellView(context: Context) : WebView(context) {
        private var capturing = true

        @SuppressLint("ClickableViewAccessibility")
        override fun dispatchTouchEvent(ev: MotionEvent): Boolean {
            if (ev.actionMasked == MotionEvent.ACTION_DOWN) {
                capturing = surfaceFull || current == null || ev.y >= height - barPx
            }
            return if (capturing) super.dispatchTouchEvent(ev) else false
        }
    }
}
