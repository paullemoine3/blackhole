//! macOS delivers links clicked in other applications to the default browser as an Apple Event
//! (`GURL`), which `winit` does not surface. This crate receives them and lets the user make
//! BlackHole the default browser. On Windows the system keeps that choice to itself: BlackHole opens
//! its page of the Settings app and reads the choice back from the registry. Elsewhere every
//! function is a harmless no-op.

#[cfg(target_os = "macos")]
mod mac {
    use objc2::{
        define_class, msg_send, rc::Retained, runtime::NSObject, sel, DefinedClass,
        MainThreadMarker, MainThreadOnly,
    };
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{
        NSAppleEventDescriptor, NSAppleEventManager, NSBundle, NSObjectProtocol, NSString, NSURL,
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

    fn bundle_path() -> Option<String> {
        let path = NSBundle::mainBundle().bundleURL().path()?.to_string();
        path.ends_with(".app").then_some(path)
    }

    pub fn can_be_default_browser() -> bool {
        bundle_path().is_some()
    }

    /// Whether the application macOS opens web links with is this bundle.
    pub fn is_default_browser() -> bool {
        let Some(own) = bundle_path() else {
            return false;
        };
        let Some(probe) = NSURL::URLWithString(&NSString::from_str("https://example.com/")) else {
            return false;
        };
        NSWorkspace::sharedWorkspace()
            .URLForApplicationToOpenURL(&probe)
            .and_then(|app| app.path())
            .is_some_and(|path| path.to_string() == own)
    }
}

/// Windows: the installer (scripts/windows-installer.iss) registers BlackHole as a browser for
/// the user (`RegisteredApplications`, ProgId `BlackHoleURL`); only the person can make it the
/// default, in Settings > Apps > Default apps, which BlackHole opens on its own page.
#[cfg(windows)]
mod win {
    use windows::core::{w, PCWSTR};
    use windows::Win32::System::Registry::{
        RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ,
    };
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    /// The ProgId the installer gives http(s) links (`Software\Classes\BlackHoleURL`).
    const PROG_ID: &str = "BlackHoleURL";

    fn read(root: HKEY, key: PCWSTR, value: PCWSTR) -> Option<String> {
        let mut buf = [0u16; 512];
        let mut size = std::mem::size_of_val(&buf) as u32;
        // SAFETY: `buf` and `size` outlive the call, `size` is the buffer's length in bytes.
        let status = unsafe {
            RegGetValueW(
                root,
                key,
                value,
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut size),
            )
        };
        if status.is_err() {
            return None;
        }
        let len = (size as usize / 2).min(buf.len());
        let text = String::from_utf16_lossy(&buf[..len]);
        Some(text.trim_end_matches(char::from(0)).to_owned())
    }

    pub fn can_be_default_browser() -> bool {
        [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE]
            .into_iter()
            .any(|root| {
                read(
                    root,
                    w!(r"Software\RegisteredApplications"),
                    w!("BlackHole"),
                )
                .is_some()
            })
    }

    pub fn is_default_browser() -> bool {
        read(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice"),
            w!("ProgId"),
        )
        .is_some_and(|id| id == PROG_ID)
    }

    /// Opens BlackHole's page of Settings > Apps > Default apps.
    pub fn set_default_browser() -> bool {
        if !can_be_default_browser() {
            return false;
        }
        // SAFETY: constant, NUL-terminated strings; no window handle.
        let done = unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                w!("ms-settings:defaultapps?registeredAppUser=BlackHole"),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        // Success is any value above 32 (the documented contract of ShellExecute).
        done.0 as isize > 32
    }
}

/// Calls `callback` (on the main thread) for every link macOS hands to the app.
pub fn register_url_handler(callback: impl Fn(String) + 'static) {
    #[cfg(target_os = "macos")]
    mac::register(Box::new(callback));
    #[cfg(not(target_os = "macos"))]
    let _ = callback;
}

/// Asks the system to make this application the default browser. macOS shows a dialog the person
/// confirms; Windows opens BlackHole's page of its Settings app, where the person chooses.
/// Returns `false` when the system cannot be asked (not an installed bundle on macOS, not
/// registered by the installer on Windows, other systems).
pub fn set_default_browser() -> bool {
    #[cfg(target_os = "macos")]
    return mac::set_default_browser();
    #[cfg(windows)]
    return win::set_default_browser();
    #[cfg(not(any(target_os = "macos", windows)))]
    false
}

/// Whether this application can be made the default browser at all (see `set_default_browser`).
pub fn can_be_default_browser() -> bool {
    #[cfg(target_os = "macos")]
    return mac::can_be_default_browser();
    #[cfg(windows)]
    return win::can_be_default_browser();
    #[cfg(not(any(target_os = "macos", windows)))]
    false
}

/// Whether the system opens web links with this application now.
pub fn is_default_browser() -> bool {
    #[cfg(target_os = "macos")]
    return mac::is_default_browser();
    #[cfg(windows)]
    return win::is_default_browser();
    #[cfg(not(any(target_os = "macos", windows)))]
    false
}
