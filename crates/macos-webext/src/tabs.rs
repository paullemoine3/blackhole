//! Exposes BlackHole's active tab and window to extensions. Without these objects an extension
//! sees no tab: uBlock Origin Lite says "not a website" and popups such as Bitwarden's wait forever.
//! A single proxy tab stands for whichever tab is currently shown.

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol, NSString, NSURL};
use objc2_web_kit::{
    WKWebExtensionContext, WKWebExtensionController, WKWebExtensionControllerDelegate,
    WKWebExtensionTab, WKWebExtensionWindow, WKWebView,
};
use std::{cell::RefCell, rc::Rc};

pub type TabObject = ProtocolObject<dyn WKWebExtensionTab>;
pub type WindowObject = ProtocolObject<dyn WKWebExtensionWindow>;

#[derive(Default)]
pub struct Shared {
    pub url: Option<Retained<NSURL>>,
    pub title: Option<Retained<NSString>>,
    pub web_view: Option<Retained<WKWebView>>,
    pub tab: Option<Retained<TabObject>>,
    pub window: Option<Retained<WindowObject>>,
}

/// Hands an object back to Objective-C (autoreleased); `None` becomes nil.
fn out<T: objc2::Message>(object: Option<Retained<T>>) -> *mut T {
    object.map_or(std::ptr::null_mut(), Retained::autorelease_return)
}

pub type State = Rc<RefCell<Shared>>;

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BHExtensionTab"]
    #[ivars = State]
    pub struct BhTab;

    unsafe impl NSObjectProtocol for BhTab {}

    unsafe impl WKWebExtensionTab for BhTab {
        #[unsafe(method(windowForWebExtensionContext:))]
        fn window(&self, _ctx: &WKWebExtensionContext) -> *mut WindowObject {
            out(self.ivars().borrow().window.clone())
        }

        #[unsafe(method(webViewForWebExtensionContext:))]
        fn web_view(&self, _ctx: &WKWebExtensionContext) -> *mut WKWebView {
            out(self.ivars().borrow().web_view.clone())
        }

        #[unsafe(method(titleForWebExtensionContext:))]
        fn title(&self, _ctx: &WKWebExtensionContext) -> *mut NSString {
            out(self.ivars().borrow().title.clone())
        }

        #[unsafe(method(urlForWebExtensionContext:))]
        fn url(&self, _ctx: &WKWebExtensionContext) -> *mut NSURL {
            out(self.ivars().borrow().url.clone())
        }

        #[unsafe(method(isLoadingCompleteForWebExtensionContext:))]
        fn is_loading_complete(&self, _ctx: &WKWebExtensionContext) -> bool {
            true
        }

        #[unsafe(method(isSelectedForWebExtensionContext:))]
        fn is_selected(&self, _ctx: &WKWebExtensionContext) -> bool {
            true
        }

        #[unsafe(method(shouldBypassPermissionsForWebExtensionContext:))]
        fn bypass_permissions(&self, _ctx: &WKWebExtensionContext) -> bool {
            true
        }
    }
);

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BHExtensionWindow"]
    #[ivars = State]
    pub struct BhWindow;

    unsafe impl NSObjectProtocol for BhWindow {}

    unsafe impl WKWebExtensionWindow for BhWindow {
        #[unsafe(method(tabsForWebExtensionContext:))]
        fn tabs(&self, _ctx: &WKWebExtensionContext) -> *mut NSArray<TabObject> {
            let tabs: Vec<_> = self.ivars().borrow().tab.iter().cloned().collect();
            out(Some(NSArray::from_retained_slice(&tabs)))
        }

        #[unsafe(method(activeTabForWebExtensionContext:))]
        fn active_tab(&self, _ctx: &WKWebExtensionContext) -> *mut TabObject {
            out(self.ivars().borrow().tab.clone())
        }
    }
);

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BHExtensionDelegate"]
    #[ivars = State]
    pub struct BhDelegate;

    unsafe impl NSObjectProtocol for BhDelegate {}

    unsafe impl WKWebExtensionControllerDelegate for BhDelegate {
        #[unsafe(method(webExtensionController:openWindowsForExtensionContext:))]
        fn open_windows(
            &self,
            _controller: &WKWebExtensionController,
            _ctx: &WKWebExtensionContext,
        ) -> *mut NSArray<WindowObject> {
            let windows: Vec<_> = self.ivars().borrow().window.iter().cloned().collect();
            out(Some(NSArray::from_retained_slice(&windows)))
        }

        #[unsafe(method(webExtensionController:focusedWindowForExtensionContext:))]
        fn focused_window(
            &self,
            _controller: &WKWebExtensionController,
            _ctx: &WKWebExtensionContext,
        ) -> *mut WindowObject {
            out(self.ivars().borrow().window.clone())
        }

        #[unsafe(method(webExtensionController:openOptionsPageForExtensionContext:completionHandler:))]
        fn open_options(
            &self,
            _controller: &WKWebExtensionController,
            _ctx: &WKWebExtensionContext,
            completion: &block2::DynBlock<dyn Fn(*mut NSError)>,
        ) {
            // Not supported yet: report success so extensions do not hang waiting for us.
            completion.call((std::ptr::null_mut(),));
        }
    }
);

/// The delegate, window and tab objects registered with the controller.
pub struct TabBridge {
    pub state: State,
    pub delegate: Retained<BhDelegate>,
    pub tab: Retained<TabObject>,
    pub window: Retained<WindowObject>,
}

impl TabBridge {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let state: State = Rc::default();
        let tab = mtm.alloc::<BhTab>().set_ivars(state.clone());
        // Safety: standard init of a class that declares no custom initialiser.
        let tab: Retained<BhTab> = unsafe { msg_send![super(tab), init] };
        let window = mtm.alloc::<BhWindow>().set_ivars(state.clone());
        let window: Retained<BhWindow> = unsafe { msg_send![super(window), init] };
        let delegate = mtm.alloc::<BhDelegate>().set_ivars(state.clone());
        let delegate: Retained<BhDelegate> = unsafe { msg_send![super(delegate), init] };
        let tab = ProtocolObject::from_retained(tab);
        let window = ProtocolObject::from_retained(window);
        {
            let mut s = state.borrow_mut();
            s.tab = Some(tab.clone());
            s.window = Some(window.clone());
        }
        Self {
            state,
            delegate,
            tab,
            window,
        }
    }
}
