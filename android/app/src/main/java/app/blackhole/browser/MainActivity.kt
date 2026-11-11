package app.blackhole.browser

import android.Manifest
import android.annotation.SuppressLint
import android.app.DownloadManager
import android.app.PendingIntent
import android.app.SearchManager
import android.app.role.RoleManager
import android.content.ActivityNotFoundException
import android.content.BroadcastReceiver
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.ApplicationInfo
import android.content.pm.PackageInstaller
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.Uri
import android.os.Bundle
import android.os.Environment
import android.os.Handler
import android.os.Looper
import android.print.PrintAttributes
import android.print.PrintManager
import android.provider.Settings
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.animation.DecelerateInterpolator
import android.webkit.CookieManager
import android.webkit.URLUtil
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebSettings
import android.webkit.WebStorage
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.FrameLayout
import androidx.activity.ComponentActivity
import androidx.activity.OnBackPressedCallback
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import androidx.core.content.IntentCompat
import androidx.core.content.pm.ShortcutInfoCompat
import androidx.core.content.pm.ShortcutManagerCompat
import androidx.core.graphics.drawable.IconCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.swiperefreshlayout.widget.SwipeRefreshLayout
import androidx.webkit.JavaScriptReplyProxy
import androidx.webkit.ProfileStore
import androidx.webkit.WebViewAssetLoader
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest
import java.util.concurrent.Executors

/**
 * The Kotlin shell: tab WebViews under one transparent interface WebView (`mobile.html`).
 * The interface only takes the touches of the areas it declares (the bottom bar, or the whole
 * screen while the start page, drawer, tab grid or a sheet is open); the rest reaches the tab.
 */
class MainActivity : ComponentActivity(), TabView.Host {

    private val main = Handler(Looper.getMainLooper())
    private lateinit var root: FrameLayout
    private lateinit var tabsLayer: FrameLayout
    /** Pull down at the top of a page to reload it; holds [tabsLayer]. */
    private lateinit var refresh: SwipeRefreshLayout
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
    /** Theme of the interface: what shows behind a page that has not drawn yet. */
    private var dark = true
    private val pageBackground get() = if (dark) 0xFF0C0B12.toInt() else 0xFFF4F3F8.toInt()
    private var insets = JSONObject()

    private val downloads = ArrayList<Long>()

    // Settings every tab view follows (`Effect::WebPrefs`).
    private var textZoom = 100
    private var pullRefresh = true

    /** A video shown full screen by a page, over everything. */
    private var fullView: View? = null
    private var fullCallback: WebChromeClient.CustomViewCallback? = null

    private var fileCallback: ValueCallback<Array<Uri>>? = null
    private val pickFiles = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        val data = result.data
        val clip = data?.clipData
        val uris = when {
            result.resultCode != RESULT_OK -> null
            clip != null -> Array(clip.itemCount) { clip.getItemAt(it).uri }
            data?.data != null -> arrayOf(data.data!!)
            else -> null
        }
        fileCallback?.onReceiveValue(uris)
        fileCallback = null
    }

    /** Site permission requests waiting for the core (the user's answer): id → (kinds, answer). */
    private val permissions = HashMap<Long, Pair<List<String>, (Boolean) -> Unit>>()
    private var permissionSeq = 0L
    private var runtimeAnswer: (() -> Unit)? = null
    private val askRuntime = registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
        val answer = runtimeAnswer
        runtimeAnswer = null
        answer?.invoke()
    }
    private val roleRequest = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { postUi(envMessage()) }

    /** Network and file work of the updater, one task at a time, off the main thread. */
    private val io = Executors.newSingleThreadExecutor()
    /** JPEG encoding of the tab grid's pictures, off the main thread. */
    private val pictures = Executors.newSingleThreadExecutor()
    private var destroyed = false
    /** The APK downloaded for the update, checked by the core before it is installed. */
    private var updateFile: File? = null
    private val density get() = resources.displayMetrics.density
    private val multiProfile by lazy { WebViewFeature.isFeatureSupported(WebViewFeature.MULTI_PROFILE) }
    private val debuggable get() = applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0

    companion object {
        private const val ORIGIN = "https://appassets.androidplatform.net"
        private const val SHELL_URL = "$ORIGIN/assets/mobile.html"
        /** Interface messages the shell handles itself; every other one goes to the core. */
        private val LOCAL = setOf(
            "surface", "thumbs", "peek", "peek_end", "copy", "share", "open_downloads",
            "find", "find_next", "find_clear", "print", "add_to_home", "default_browser",
        )
        private const val INSTALL_STATUS = "app.blackhole.browser.INSTALL_STATUS"
        /** The APK is a few tens of megabytes; anything far bigger is not ours. */
        private const val MAX_APK = 200L * 1024 * 1024
    }

    private val agent by lazy { "BlackHole/" + (packageManager.getPackageInfo(packageName, 0).versionName ?: "") }

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        WindowCompat.setDecorFitsSystemWindows(window, false)
        if (debuggable) WebView.setWebContentsDebuggingEnabled(true)

        root = FrameLayout(this)
        root.setBackgroundColor(0xFF0C0B12.toInt())
        tabsLayer = FrameLayout(this)
        refresh = SwipeRefreshLayout(this)
        refresh.addView(tabsLayer, ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        refresh.setColorSchemeColors(0xFF7C5CFF.toInt(), 0xFFFFB020.toInt())
        refresh.setProgressBackgroundColorSchemeColor(0xFF13121B.toInt())
        // Only at the very top of a page that does not scroll up any more.
        refresh.setOnChildScrollUpCallback { _, _ ->
            val v = current?.let { tabs[it] }
            !pullRefresh || v == null || fullView != null || v.scrollY > 0 || v.canScrollVertically(-1)
        }
        refresh.setOnRefreshListener {
            apply(Native.command("""{"cmd":"reload"}"""))
            main.postDelayed({ refresh.isRefreshing = false }, 1500)
        }
        root.addView(refresh, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        shell = ShellView(this)
        root.addView(shell, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        setContentView(root)

        ViewCompat.setOnApplyWindowInsetsListener(root) { _, all ->
            val bars = all.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
            val ime = all.getInsets(WindowInsetsCompat.Type.ime())
            insets = JSONObject()
                .put("type", "insets")
                .put("top", bars.top / density)
                // The keyboard apart from the system bars: the interface decides what it lifts
                // (the bar under a page's keyboard, nothing under its own address field).
                .put("bottom", bars.bottom / density)
                .put("left", bars.left / density)
                .put("right", bars.right / density)
                .put("keyboard", ime.bottom / density)
            val params = refresh.layoutParams as FrameLayout.LayoutParams
            if (params.topMargin != bars.top) {
                params.topMargin = bars.top
                refresh.requestLayout()
            }
            postUi(insets.toString())
            all
        }

        ContextCompat.registerReceiver(this, installStatus, IntentFilter(INSTALL_STATUS), ContextCompat.RECEIVER_NOT_EXPORTED)
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
            .put("default_browser", isDefaultBrowser())
            .toString()
    }

    private fun isDefaultBrowser(): Boolean {
        val roles = getSystemService(RoleManager::class.java) ?: return false
        return roles.isRoleAvailable(RoleManager.ROLE_BROWSER) && roles.isRoleHeld(RoleManager.ROLE_BROWSER)
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
                // A page is laid out again only when the bar's height really changed.
                val params = refresh.layoutParams as FrameLayout.LayoutParams
                if (params.bottomMargin != barPx) {
                    params.bottomMargin = barPx
                    refresh.requestLayout()
                }
                val nowDark = msg.optBoolean("dark", true)
                WindowCompat.getInsetsController(window, root).apply {
                    if (isAppearanceLightStatusBars == nowDark) isAppearanceLightStatusBars = !nowDark
                    if (isAppearanceLightNavigationBars == nowDark) isAppearanceLightNavigationBars = !nowDark
                }
                if (nowDark != dark) {
                    dark = nowDark
                    root.setBackgroundColor(pageBackground)
                    tabs.values.forEach { it.setBackgroundColor(pageBackground) }
                }
                val visibility = if (msg.optBoolean("cover", false)) View.INVISIBLE else View.VISIBLE
                if (tabsLayer.visibility != visibility) tabsLayer.visibility = visibility
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
            "find" -> currentView()?.findAllAsync(msg.optString("text").take(500))
            "find_next" -> currentView()?.findNext(msg.optBoolean("forward", true))
            "find_clear" -> currentView()?.clearMatches()
            "print" -> currentView()?.let { printPage(it) }
            "add_to_home" -> currentView()?.let { addToHome(it) }
            "default_browser" -> askDefaultBrowser()
        }
    }

    private fun currentView(): TabView? = current?.let { tabs[it] }

    /** Print, or "Save as PDF" in the system dialog. */
    private fun printPage(view: TabView) {
        val name = view.title?.takeIf { it.isNotBlank() } ?: "BlackHole"
        val printer = getSystemService(Context.PRINT_SERVICE) as PrintManager
        printer.print(name, view.createPrintDocumentAdapter(name), PrintAttributes.Builder().build())
    }

    /** A launcher shortcut that opens the page in BlackHole. */
    private fun addToHome(view: TabView) {
        val url = view.url ?: return
        if (!URLUtil.isNetworkUrl(url)) return
        if (!ShortcutManagerCompat.isRequestPinShortcutSupported(this)) {
            toast("Your home screen does not accept shortcuts")
            return
        }
        val label = view.title?.takeIf { it.isNotBlank() } ?: Uri.parse(url).host ?: "BlackHole"
        val icon = view.pageIcon?.let { IconCompat.createWithBitmap(shortcutIcon(it)) }
            ?: IconCompat.createWithResource(this, R.mipmap.ic_launcher)
        val open = Intent(Intent.ACTION_VIEW, Uri.parse(url)).setClass(this, MainActivity::class.java)
        val info = ShortcutInfoCompat.Builder(this, "site-" + Integer.toHexString(url.hashCode()))
            .setShortLabel(label.take(24))
            .setLongLabel(label.take(60))
            .setIcon(icon)
            .setIntent(open)
            .build()
        ShortcutManagerCompat.requestPinShortcut(this, info, null)
    }

    /** The site icon centred on a light rounded tile (launcher icons are 192 px). */
    private fun shortcutIcon(favicon: Bitmap): Bitmap {
        val side = 192
        val out = Bitmap.createBitmap(side, side, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(out)
        val paint = Paint(Paint.ANTI_ALIAS_FLAG or Paint.FILTER_BITMAP_FLAG)
        paint.color = 0xFFF4F3F8.toInt()
        canvas.drawRoundRect(RectF(0f, 0f, side.toFloat(), side.toFloat()), 44f, 44f, paint)
        val inset = side * 0.24f
        canvas.drawBitmap(favicon, null, RectF(inset, inset, side - inset, side - inset), paint)
        return out
    }

    private fun askDefaultBrowser() {
        val roles = getSystemService(RoleManager::class.java)
        if (roles != null && roles.isRoleAvailable(RoleManager.ROLE_BROWSER)) {
            if (roles.isRoleHeld(RoleManager.ROLE_BROWSER)) {
                toast("BlackHole is already your default browser")
            } else {
                roleRequest.launch(roles.createRequestRoleIntent(RoleManager.ROLE_BROWSER))
            }
            return
        }
        try {
            startActivity(Intent(Settings.ACTION_MANAGE_DEFAULT_APPS_SETTINGS))
        } catch (e: ActivityNotFoundException) {
            toast("Choose the default browser in the system settings")
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
                    it.setBackgroundColor(pageBackground)
                    it.settings.textZoom = textZoom
                    tabsLayer.addView(it, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
                    tabs[tab] = it
                }
                view.setDesktop(e.optBoolean("desktop"), reload = false)
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
                    "stop" -> v.stopLoading()
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
            "desktop_mode" -> tabs[tab]?.setDesktop(e.optBoolean("on"), reload = true)
            "web_prefs" -> {
                textZoom = e.optInt("text_zoom", 100)
                pullRefresh = e.optBoolean("pull_refresh", true)
                tabs.values.forEach { it.settings.textZoom = textZoom }
            }
            "permission" -> answerPermission(e.optLong("id"), e.optBoolean("allow"))
            // "Download link" of the long press menu (the page's own downloads go through download()).
            "download" -> enqueue(
                e.optString("url"), WebSettings.getDefaultUserAgent(this), e.optString("disposition"),
                e.optString("mime"), currentView()?.profile ?: "Default",
            )
            "update_probe" -> updateProbe(e.optString("url"))
            "update_sums" -> updateSums(e.optString("tag"), e.optString("url"))
            "update_download" -> updateDownload(e.optString("url"), e.optString("sha256"), e.optString("name"))
            "update_install" -> updateInstall()
            "update_discard" -> {
                updateFile?.delete()
                updateFile = null
            }
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
        // The bar's progress line belongs to the page on screen: without this, the line of a
        // page left while loading stayed on the next one.
        postProgress(tab, next?.progress ?: 100)
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

    /** The page is drawn here (it must be), then compressed and encoded on another thread. */
    private fun postThumb(view: TabView) {
        val picture = view.snapshot() ?: return
        val tab = view.tab
        pictures.execute {
            val data = TabView.encode(picture) ?: return@execute
            main.post {
                if (!destroyed) postUi(JSONObject().put("type", "thumb").put("tab", tab).put("data", data).toString())
            }
        }
    }

    private fun postProgress(tab: Long?, value: Int) {
        postUi(JSONObject().put("type", "progress").put("tab", tab ?: JSONObject.NULL).put("value", value).toString())
    }

    private fun back() {
        val view = current?.let { tabs[it] }
        when {
            fullView != null -> leaveFullscreen()
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
        if (tab != current) return
        postProgress(tab, value)
        if (value >= 100) refresh.isRefreshing = false
    }

    override fun found(tab: Long, active: Int, count: Int) {
        if (tab == current) postUi(JSONObject().put("type", "found").put("active", active).put("count", count).toString())
    }

    override fun fullscreen(view: View, callback: WebChromeClient.CustomViewCallback) {
        if (fullView != null) {
            callback.onCustomViewHidden()
            return
        }
        fullView = view
        fullCallback = callback
        view.setBackgroundColor(Color.BLACK)
        root.addView(view, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        WindowCompat.getInsetsController(window, root).apply {
            systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            hide(WindowInsetsCompat.Type.systemBars())
        }
    }

    /** The page left full screen (`onHideCustomView`). */
    override fun exitFullscreen() {
        val view = fullView ?: return
        fullView = null
        fullCallback = null
        root.removeView(view)
        WindowCompat.getInsetsController(window, root).show(WindowInsetsCompat.Type.systemBars())
    }

    /** Back during full screen: the page is told, then the view goes. */
    private fun leaveFullscreen() {
        val callback = fullCallback
        exitFullscreen()
        callback?.onCustomViewHidden()
    }

    override fun chooseFiles(callback: ValueCallback<Array<Uri>>, params: WebChromeClient.FileChooserParams): Boolean {
        fileCallback?.onReceiveValue(null)
        fileCallback = callback
        val intent = params.createIntent()
        if (params.mode == WebChromeClient.FileChooserParams.MODE_OPEN_MULTIPLE) intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
        return try {
            pickFiles.launch(intent)
            true
        } catch (e: ActivityNotFoundException) {
            fileCallback = null
            toast("No app can pick a file")
            false
        }
    }

    override fun askPermission(tab: Long, origin: String, kinds: List<String>, answer: (Boolean) -> Unit) {
        val id = ++permissionSeq
        permissions[id] = kinds to answer
        val json = JSONObject().put("ev", "permission").put("id", id).put("tab", tab).put("origin", origin).put("kinds", JSONArray(kinds))
        event(json)
    }

    private fun systemPermissions(kind: String): List<String> = when (kind) {
        "camera" -> listOf(Manifest.permission.CAMERA)
        "microphone" -> listOf(Manifest.permission.RECORD_AUDIO)
        "location" -> listOf(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION)
        else -> emptyList()
    }

    private fun hasKind(kind: String): Boolean = systemPermissions(kind).let { needed ->
        needed.isEmpty() || needed.any { ContextCompat.checkSelfPermission(this, it) == PackageManager.PERMISSION_GRANTED }
    }

    /** The core's answer; an allowed site still needs the app's own system permission. */
    private fun answerPermission(id: Long, allow: Boolean) {
        val (kinds, answer) = permissions.remove(id) ?: return
        if (!allow) return answer(false)
        val missing = kinds.filterNot { hasKind(it) }.flatMap { systemPermissions(it) }
        if (missing.isEmpty()) return answer(true)
        if (runtimeAnswer != null) return answer(false)
        runtimeAnswer = { answer(kinds.all { hasKind(it) }) }
        askRuntime.launch(missing.toTypedArray())
    }

    override fun navigated(tab: Long) {
        val v = tabs[tab] ?: return
        if (tab != current) return
        postUi(JSONObject().put("type", "nav").put("tab", tab).put("back", v.canGoBack()).put("forward", v.canGoForward()).toString())
    }

    /** The tab whose shield count is due (see [blocked]). */
    private var blockedTab = -1L
    private val refreshBlocked = Runnable {
        apply(Native.event(JSONObject().put("ev", "blocked").put("tab", blockedTab).toString()))
    }

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
        // At most one count every half second, however many requests are refused; the count
        // alone, not the whole state (and every site icon) the interface would draw again.
        main.post {
            if (tab == current && !main.hasCallbacks(refreshBlocked)) {
                blockedTab = tab
                main.postDelayed(refreshBlocked, 500)
            }
        }
    }

    override fun linkMenu(tab: Long, url: String?, image: String?) {
        val link = url?.takeIf { URLUtil.isNetworkUrl(it) }
        val img = image?.takeIf { URLUtil.isNetworkUrl(it) }
        if (link == null && img == null) return
        postUi(JSONObject().put("type", "link_menu").put("url", link ?: JSONObject.NULL).put("image", img ?: JSONObject.NULL).toString())
    }

    // Updates: the core decides, this shell fetches (Android's trust store) and installs.

    private fun coreEvent(json: JSONObject) {
        main.post { apply(Native.event(json.toString())) }
    }

    private fun updateError(e: Exception) =
        coreEvent(JSONObject().put("ev", "update_failed").put("error", e.message ?: e.javaClass.simpleName))

    /** GitHub only, over https; redirects (to the release files host) stay on https. */
    private fun connect(url: String, method: String = "GET"): HttpURLConnection {
        if (!url.startsWith("https://github.com/")) throw IOException("Refused address")
        val c = URL(url).openConnection() as HttpURLConnection
        c.requestMethod = method
        c.instanceFollowRedirects = true
        c.connectTimeout = 15000
        c.readTimeout = 60000
        c.setRequestProperty("User-Agent", agent)
        val code = c.responseCode
        if (!c.url.toString().startsWith("https://")) throw IOException("Insecure redirect")
        if (code !in 200..299) throw IOException("HTTP $code")
        return c
    }

    private fun updateProbe(url: String) = io.execute {
        try {
            val c = connect(url, "HEAD")
            val final = c.url.toString()
            c.disconnect()
            coreEvent(JSONObject().put("ev", "update_latest").put("url", final))
        } catch (e: IOException) {
            updateError(e)
        }
    }

    private fun updateSums(tag: String, url: String) = io.execute {
        try {
            val c = connect(url)
            val bytes = c.inputStream.use { input ->
                val out = java.io.ByteArrayOutputStream()
                val buffer = ByteArray(16 * 1024)
                while (out.size() <= 1 shl 20) {
                    val n = input.read(buffer)
                    if (n < 0) break
                    out.write(buffer, 0, n)
                }
                if (out.size() > 1 shl 20) throw IOException("Checksums too large")
                out.toByteArray()
            }
            c.disconnect()
            coreEvent(JSONObject().put("ev", "update_sums").put("tag", tag).put("sums", String(bytes, Charsets.UTF_8)).put("debug", debuggable))
        } catch (e: IOException) {
            updateError(e)
        }
    }

    private fun sha256(file: File): String = file.inputStream().use { input ->
        val digest = MessageDigest.getInstance("SHA-256")
        val buffer = ByteArray(64 * 1024)
        while (true) {
            val n = input.read(buffer)
            if (n < 0) break
            digest.update(buffer, 0, n)
        }
        digest.digest().joinToString("") { "%02x".format(it) }
    }

    /** Downloads the APK (or reuses the one already here), then reports its SHA-256 to the core. */
    private fun updateDownload(url: String, expected: String, name: String) {
        if (!name.matches(Regex("[A-Za-z0-9._-]+\\.apk"))) return
        val dir = File(cacheDir, "updates").apply { mkdirs() }
        val file = File(dir, name)
        updateFile = file
        io.execute {
            try {
                if (!(file.isFile && sha256(file).equals(expected, ignoreCase = true))) {
                    dir.listFiles()?.forEach { it.delete() }
                    val c = connect(url)
                    val total = c.contentLengthLong
                    if (total > MAX_APK) throw IOException("Download too large")
                    val digest = MessageDigest.getInstance("SHA-256")
                    var done = 0L
                    var shown = 0L
                    c.inputStream.use { input ->
                        file.outputStream().use { out ->
                            val buffer = ByteArray(64 * 1024)
                            while (true) {
                                val n = input.read(buffer)
                                if (n < 0) break
                                done += n
                                if (done > MAX_APK) throw IOException("Download too large")
                                digest.update(buffer, 0, n)
                                out.write(buffer, 0, n)
                                val now = System.currentTimeMillis()
                                if (now - shown > 250) {
                                    shown = now
                                    val d = done
                                    main.post { postUi(JSONObject().put("type", "update_progress").put("done", d).put("total", total).toString()) }
                                }
                            }
                        }
                    }
                    c.disconnect()
                }
                coreEvent(JSONObject().put("ev", "update_downloaded").put("sha256", sha256(file)))
            } catch (e: IOException) {
                file.delete()
                updateError(e)
            }
        }
    }

    /** Hands the checked APK to the system installer, which asks the user to confirm. */
    private fun updateInstall() {
        val file = updateFile?.takeIf { it.isFile } ?: return updateError(IOException("The download is gone"))
        if (!packageManager.canRequestPackageInstalls()) {
            updateError(IOException("Allow BlackHole to install apps, then tap Install again"))
            try {
                startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:$packageName")))
            } catch (e: ActivityNotFoundException) {
                // The error says what to allow.
            }
            return
        }
        io.execute {
            try {
                val installer = packageManager.packageInstaller
                val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL)
                params.setAppPackageName(packageName)
                val id = installer.createSession(params)
                installer.openSession(id).use { session ->
                    session.openWrite("blackhole.apk", 0, file.length()).use { out ->
                        file.inputStream().use { it.copyTo(out) }
                        session.fsync(out)
                    }
                    val flags = PendingIntent.FLAG_UPDATE_CURRENT or
                        (if (android.os.Build.VERSION.SDK_INT >= 31) PendingIntent.FLAG_MUTABLE else 0)
                    val status = PendingIntent.getBroadcast(this, id, Intent(INSTALL_STATUS).setPackage(packageName), flags)
                    session.commit(status.intentSender)
                }
            } catch (e: IOException) {
                updateError(e)
            } catch (e: SecurityException) {
                coreEvent(JSONObject().put("ev", "update_failed").put("error", e.message ?: "Installation refused"))
            }
        }
    }

    /** What the installer says: ask the user, or why it failed. */
    private val installStatus = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            when (val status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)) {
                PackageInstaller.STATUS_PENDING_USER_ACTION -> {
                    val confirm = IntentCompat.getParcelableExtra(intent, Intent.EXTRA_INTENT, Intent::class.java) ?: return
                    try {
                        startActivity(confirm)
                    } catch (e: ActivityNotFoundException) {
                        updateError(IOException("No installer"))
                    }
                }
                PackageInstaller.STATUS_SUCCESS -> Unit // The app is replaced (and restarted by the user).
                else -> {
                    val why = when (status) {
                        PackageInstaller.STATUS_FAILURE_ABORTED -> "Installation cancelled"
                        PackageInstaller.STATUS_FAILURE_CONFLICT, PackageInstaller.STATUS_FAILURE_INCOMPATIBLE ->
                            "This APK is signed with another key: uninstall BlackHole first"
                        PackageInstaller.STATUS_FAILURE_STORAGE -> "Not enough storage"
                        else -> intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE) ?: "Installation failed"
                    }
                    coreEvent(JSONObject().put("ev", "update_failed").put("error", why))
                }
            }
        }
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
                        .put("id", c.getLong(c.getColumnIndexOrThrow(DownloadManager.COLUMN_ID)))
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
        // Text shared with BlackHole, a web search, or "BlackHole" in a text selection menu.
        val shared = when (intent.action) {
            Intent.ACTION_SEND -> intent.getStringExtra(Intent.EXTRA_TEXT)
            Intent.ACTION_WEB_SEARCH -> intent.getStringExtra(SearchManager.QUERY)
            Intent.ACTION_PROCESS_TEXT -> intent.getCharSequenceExtra(Intent.EXTRA_PROCESS_TEXT)?.toString()
            else -> null
        }
        if (!shared.isNullOrBlank()) {
            apply(Native.event(JSONObject().put("ev", "shared").put("text", shared.take(2000)).toString()))
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
        apply(Native.event("""{"ev":"resume"}"""))
    }

    override fun onDestroy() {
        destroyed = true
        main.removeCallbacks(refreshBlocked)
        unregisterReceiver(installStatus)
        io.shutdown()
        pictures.shutdown()
        fileCallback?.onReceiveValue(null)
        fileCallback = null
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
