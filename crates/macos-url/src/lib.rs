//! macOS delivers links clicked in other applications to the default browser as an Apple Event
//! (`GURL`), which `winit` does not surface. This crate receives them and lets the user make
//! BlackHole the default browser. On other platforms every function is a harmless no-op.

#[cfg(target_os = "macos")]
mod mac {
    use objc2::{
        define_class, msg_send, rc::Retained, runtime::NSObject, sel, DefinedClass,
        MainThreadMarker, MainThreadOnly,
    };
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{
        NSAppleEventDescriptor, NSAppleEventManager, NSBundle, NSObjectProtocol, NSString,
    };

    /// `kInternetEventClass` and `kAEGetURL` are both the four-char code 'GURL'.
    const GURL: u32 = 0x4755_524C;
    /// `keyDirectObject`: '----'.
    const DIRECT_OBJECT: u32 = 0x2D2D_2D2D;

    pub struct Ivars {
        callback: Box<dyn Fn(String)>,
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "BlackHoleUrlHandler"]
        #[ivars = Ivars]
        struct Handler;

        impl Handler {
            #[unsafe(method(handleURL:withReplyEvent:))]
            fn handle_url(&self, event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
                let direct: Option<Retained<NSAppleEventDescriptor>> =
                    unsafe { msg_send![event, paramDescriptorForKeyword: DIRECT_OBJECT] };
                let url = direct.and_then(|d| d.stringValue()).map(|s| s.to_string());
                if let Some(url) = url {
                    (self.ivars().callback)(url);
                }
            }
        }

        unsafe impl NSObjectProtocol for Handler {}
    );

    impl Handler {
        fn new(mtm: MainThreadMarker, callback: Box<dyn Fn(String)>) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(Ivars { callback });
            unsafe { msg_send![super(this), init] }
        }
    }

    pub fn register(callback: Box<dyn Fn(String)>) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let handler = Handler::new(mtm, callback);
        let manager = NSAppleEventManager::sharedAppleEventManager();
        unsafe {
            let _: () = msg_send![
                &*manager,
                setEventHandler: &*handler,
                andSelector: sel!(handleURL:withReplyEvent:),
                forEventClass: GURL,
                andEventID: GURL,
            ];
        }
        // The Apple Event manager does not retain its handler.
        std::mem::forget(handler);
    }

    pub fn set_default_browser() -> bool {
        let bundle = NSBundle::mainBundle();
        let url = bundle.bundleURL();
        let is_app = url.path().is_some_and(|p| p.to_string().ends_with(".app"));
        if !is_app {
            return false;
        }
        let workspace = NSWorkspace::sharedWorkspace();
        for scheme in ["http", "https"] {
            workspace.setDefaultApplicationAtURL_toOpenURLsWithScheme_completionHandler(
                &url,
                &NSString::from_str(scheme),
                None,
            );
        }
        true
    }
}

/// Calls `callback` (on the main thread) for every link macOS hands to the app.
pub fn register_url_handler(callback: impl Fn(String) + 'static) {
    #[cfg(target_os = "macos")]
    mac::register(Box::new(callback));
    #[cfg(not(target_os = "macos"))]
    let _ = callback;
}

/// Asks the system to make this application the default browser (the user confirms in a dialog).
/// Returns `false` when not running from an installed bundle.
pub fn set_default_browser() -> bool {
    #[cfg(target_os = "macos")]
    return mac::set_default_browser();
    #[cfg(not(target_os = "macos"))]
    false
}
