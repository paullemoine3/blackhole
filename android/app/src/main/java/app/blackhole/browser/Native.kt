package app.blackhole.browser

/**
 * The Rust core (`crates/android`). Every call but [shouldBlock] runs on the main thread;
 * messages and answers are JSON (see `crates/android/src/bridge.rs`).
 */
object Native {
    init {
        System.loadLibrary("blackhole_android")
    }

    /**
     * Opens the profile in [dataDir] and starts the paired devices service: [vaultKey] (hex,
     * from [KeyVault]) seals the pairing keys, [deviceName] is shown on the other devices,
     * [peer] (`ip:port`, debug builds only) replaces the local network. Returns the first effects.
     */
    external fun init(dataDir: String, vaultKey: String, deviceName: String, peer: String): String

    /** A `MobileCommand` of the shell interface (trusted page of our assets). */
    external fun command(json: String): String

    /** A `NativeEvent` of this shell. */
    external fun event(json: String): String

    /** A `PageEvent` posted by the page of [tab] (untrusted). */
    external fun pageEvent(tab: Long, json: String): String

    /** Whether to refuse a subresource request; called from WebView network threads. */
    external fun shouldBlock(url: String, page: String): Boolean
}
