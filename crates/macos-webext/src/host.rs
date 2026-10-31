use super::tabs::TabBridge;
use super::{ActionInfo, LoadState};
use base64::Engine;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyClass;
use objc2::runtime::ProtocolObject;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSPopover, NSPopoverBehavior, NSView,
};
use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSRectEdge, NSSize};
use objc2_foundation::{NSError, NSString, NSURL};
use objc2_web_kit::{
    WKUserScript, WKUserScriptInjectionTime, WKWebExtension, WKWebExtensionContext,
    WKWebExtensionContextPermissionStatus, WKWebExtensionController, WKWebExtensionMatchPattern,
    WKWebExtensionControllerConfiguration, WKWebExtensionTabChangedProperties, WKWebView,
    WKWebViewConfiguration,
};
use std::{cell::RefCell, collections::HashMap, path::Path, rc::Rc};
use std::{ffi::c_void, ptr::NonNull};

type Contexts = Rc<RefCell<HashMap<String, Retained<WKWebExtensionContext>>>>;
type States = Rc<RefCell<HashMap<String, LoadState>>>;

/// Owns the extension controller. Must live as long as the web views that use it.
pub struct ExtensionHost {
    mtm: MainThreadMarker,
    controller: Retained<WKWebExtensionController>,
    contexts: Contexts,
    states: States,
    popover: RefCell<Option<Retained<NSPopover>>>,
    bridge: TabBridge,
}

impl ExtensionHost {
    /// `None` off the main thread or on a macOS that predates the public extension API.
    pub fn new() -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        AnyClass::get(c"WKWebExtensionController")?;
        // Safety: plain Objective-C object construction on the main thread.
        let controller = unsafe {
            let config = WKWebExtensionControllerConfiguration::defaultConfiguration(mtm);
            // Background pages and popups sniff the user agent; WebKit's default has no browser name and Bitwarden then crashes. The Firefox name matches the MV2 Firefox build in the catalog (a Safari one makes it wait for a native app).
            let web = WKWebViewConfiguration::new(mtm);
            web.setApplicationNameForUserAgent(Some(&NSString::from_str("Firefox/128.0")));
            // Extension pages get an emulated `privacy` API (see privacy_shim.js).
            let shim = WKUserScript::initWithSource_injectionTime_forMainFrameOnly(
                mtm.alloc(),
                &NSString::from_str(include_str!("privacy_shim.js")),
                WKUserScriptInjectionTime::AtDocumentStart,
                false,
            );
            web.userContentController().addUserScript(&shim);
            config.setWebViewConfiguration(Some(&web));
            WKWebExtensionController::initWithConfiguration(mtm.alloc(), &config)
        };
        let bridge = TabBridge::new(mtm);
        // Safety: the delegate is kept alive by `bridge` (the controller holds it weakly).
        unsafe {
            controller.setDelegate(Some(ProtocolObject::from_ref(&*bridge.delegate)));
            controller.didOpenWindow(&bridge.window);
            controller.didOpenTab(&bridge.tab);
            controller.didFocusWindow(Some(&bridge.window));
            controller.didActivateTab_previousActiveTab(&bridge.tab, None);
        }
        Some(Self {
            mtm,
            controller,
            contexts: Rc::default(),
            states: Rc::default(),
            popover: RefCell::new(None),
            bridge,
        })
    }

    /// A fresh web view configuration wired to the controller: pass it to every page view so
    /// extensions apply to it.
    pub fn view_configuration(&self) -> Retained<WKWebViewConfiguration> {
        // Safety: setting a property on an object we just created.
        unsafe {
            let config = WKWebViewConfiguration::new(self.mtm);
            config.setWebExtensionController(Some(&self.controller));
            config
        }
    }

    /// Starts loading an unpacked extension folder or a `.zip`. Loading is asynchronous; poll
    /// [`state`](Self::state). Grants everything the extension asks for: the user installed it.
    pub fn load(&self, id: &str, path: &Path) {
        self.unload(id);
        self.states
            .borrow_mut()
            .insert(id.to_owned(), LoadState::Loading);
        let Some(url) = path
            .to_str()
            .map(NSString::from_str)
            .map(|p| NSURL::fileURLWithPath(&p))
        else {
            self.states
                .borrow_mut()
                .insert(id.to_owned(), LoadState::Failed("invalid path".into()));
            return;
        };
        let (controller, contexts, states, key) = (
            self.controller.clone(),
            self.contexts.clone(),
            self.states.clone(),
            id.to_owned(),
        );
        let block = RcBlock::new(move |ext: *mut WKWebExtension, err: *mut NSError| {
            let outcome = if ext.is_null() {
                // Safety: the framework passes a valid NSError when the extension is nil.
                let reason = unsafe { err.as_ref() }.map_or_else(
                    || "unknown error".to_owned(),
                    |e| e.localizedDescription().to_string(),
                );
                Err(reason)
            } else {
                // Safety: non-null, valid for the duration of the callback.
                let ext = unsafe { &*ext };
                activate(&controller, ext, &key).map(|ctx| {
                    contexts.borrow_mut().insert(key.clone(), ctx);
                })
            };
            let state = outcome.map_or_else(LoadState::Failed, |()| LoadState::Loaded);
            states.borrow_mut().insert(key.clone(), state);
        });
        // Safety: the block is copied by the framework; the URL is a valid file URL.
        unsafe {
            WKWebExtension::extensionWithResourceBaseURL_completionHandler(&url, &block, self.mtm)
        };
    }

    pub fn unload(&self, id: &str) {
        if let Some(ctx) = self.contexts.borrow_mut().remove(id) {
            // Safety: the context was loaded by this controller.
            let _ = unsafe { self.controller.unloadExtensionContext_error(&ctx) };
        }
        self.states.borrow_mut().remove(id);
    }

    /// Toolbar buttons of the loaded extensions that declare an action.
    pub fn actions(&self) -> Vec<ActionInfo> {
        let contexts = self.contexts.borrow();
        let mut ids: Vec<_> = contexts.keys().cloned().collect();
        ids.sort();
        ids.into_iter()
            .filter_map(|id| {
                // Safety: accessors on live objects, main thread.
                unsafe {
                    let action = contexts[&id].actionForTab(Some(&self.bridge.tab))?;
                    let icon = action.iconForSize(NSSize::new(32.0, 32.0)).and_then(|img| {
                        let tiff = img.TIFFRepresentation()?;
                        let rep = NSBitmapImageRep::imageRepWithData(&tiff)?;
                        let png = rep.representationUsingType_properties(
                            NSBitmapImageFileType::PNG,
                            &NSDictionary::new(),
                        )?;
                        Some(format!(
                            "data:image/png;base64,{}",
                            base64::engine::general_purpose::STANDARD.encode(png.to_vec())
                        ))
                    });
                    Some(ActionInfo {
                        label: action.label().to_string(),
                        badge: action.badgeText().to_string(),
                        enabled: action.isEnabled(),
                        icon,
                        id,
                    })
                }
            })
            .collect()
    }

    /// Opens the extension's popup under `rect` (x, y, w, h in the content view's top-left
    /// coordinates), or runs its click action when it has no popup. `view` must be the window's
    /// content `NSView` (from its raw window handle).
    pub fn show_popup(&self, id: &str, view: NonNull<c_void>, rect: (f64, f64, f64, f64)) -> bool {
        let Some(ctx) = self.contexts.borrow().get(id).cloned() else {
            return false;
        };
        // Safety: `view` is a live NSView owned by the window for the duration of the call; all
        // other objects are retained Objective-C objects used on the main thread.
        unsafe {
            let Some(action) = ctx.actionForTab(Some(&self.bridge.tab)) else {
                return false;
            };
            if !action.presentsPopup() {
                ctx.performActionForTab(Some(&self.bridge.tab));
                return true;
            }
            let Some(popover) = action.popupPopover() else {
                return false;
            };
            let view: &NSView = view.cast::<NSView>().as_ref();
            popover.setBehavior(NSPopoverBehavior::Transient);
            let (x, y, w, h) = rect;
            let flipped_y = if view.isFlipped() {
                y
            } else {
                view.bounds().size.height - y - h
            };
            let edge = if view.isFlipped() {
                NSRectEdge::MaxY
            } else {
                NSRectEdge::MinY
            };
            popover.showRelativeToRect_ofView_preferredEdge(
                NSRect::new(NSPoint::new(x, flipped_y), NSSize::new(w, h)),
                view,
                edge,
            );
            *self.popover.borrow_mut() = Some(popover);
            true
        }
    }

    /// Tells extensions which page is in front. `web_view` is the page's `WKWebView`.
    pub fn set_active_tab(&self, web_view: Option<NonNull<c_void>>, url: &str, title: &str) {
        // Safety: the pointer is a live WKWebView on the main thread; it is retained here.
        let web_view =
            web_view.and_then(|p| unsafe { Retained::retain(p.cast::<WKWebView>().as_ptr()) });
        let url = NSURL::URLWithString(&NSString::from_str(url));
        let title = NSString::from_str(title);
        let mut changed = WKWebExtensionTabChangedProperties::None;
        {
            let mut s = self.bridge.state.borrow_mut();
            if s.url != url {
                changed |= WKWebExtensionTabChangedProperties::URL;
            }
            if s.title.as_deref() != Some(&*title) {
                changed |= WKWebExtensionTabChangedProperties::Title;
            }
            s.url = url;
            s.title = Some(title);
            s.web_view = web_view;
        }
        if changed != WKWebExtensionTabChangedProperties::None {
            // Safety: the tab was registered with this controller in `new`.
            unsafe {
                self.controller
                    .didChangeTabProperties_forTab(changed, &self.bridge.tab)
            };
        }
    }

    /// Temporary: prints popup clicks/errors/network calls (never field values) to stderr.
    pub fn state(&self, id: &str) -> Option<LoadState> {
        self.states.borrow().get(id).cloned()
    }
}

fn activate(
    controller: &WKWebExtensionController,
    ext: &WKWebExtension,
    id: &str,
) -> Result<Retained<WKWebExtensionContext>, String> {
    // Safety: all calls below are plain property/permission accessors on live objects.
    unsafe {
        let ctx = WKWebExtensionContext::contextForExtension(ext);
        // A stable identifier keeps the extension's storage across launches.
        ctx.setUniqueIdentifier(&NSString::from_str(id));
        // Extensions only trust `safari-web-extension:` (like `chrome-extension:`) when validating
        // their own frame URLs, e.g. Bitwarden's inline menu container. Service-worker extensions
        // (Dark Reader, uBlock Origin Lite) must keep the default base URL: with a custom one
        // WebKit stops delivering content-script messages to their worker.
        if let (Some(mtm), true) = (
            objc2::MainThreadMarker::new(),
            ext.hasPersistentBackgroundContent(),
        ) {
            WKWebExtensionMatchPattern::registerCustomURLScheme(
                &NSString::from_str("safari-web-extension"),
                mtm,
            );
            if let Some(base) =
                NSURL::URLWithString(&NSString::from_str(&format!("safari-web-extension://{id}")))
            {
                ctx.setBaseURL(&base);
            }
        }
        let granted = WKWebExtensionContextPermissionStatus::GrantedExplicitly;
        for permission in ext.requestedPermissions().iter() {
            ctx.setPermissionStatus_forPermission(granted, &permission);
        }
        for pattern in ext.allRequestedMatchPatterns().iter() {
            ctx.setPermissionStatus_forMatchPattern(granted, &pattern);
        }
        ctx.setHasRequestedOptionalAccessToAllHosts(true);
        controller
            .loadExtensionContext_error(&ctx)
            .map_err(|e| e.localizedDescription().to_string())?;
        Ok(ctx)
    }
}
