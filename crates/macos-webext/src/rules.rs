//! Native request blocking on macOS: host lists compiled into WebKit content rule lists
//! (`WKContentRuleList`). The engine enforces them in every frame, for every kind of request,
//! without a single script in the page.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_foundation::{NSError, NSString};
use objc2_web_kit::{
    WKContentRuleList, WKContentRuleListStore, WKUserContentController, WKWebView,
};
use std::{cell::RefCell, collections::HashMap, ffi::c_void, ptr::NonNull};

enum List {
    /// Being compiled: the views created meanwhile get it once it is ready.
    Compiling(Vec<Retained<WKUserContentController>>),
    Ready(Retained<WKContentRuleList>),
    /// Empty, or refused by WebKit (reported once on stderr).
    Unusable,
}

thread_local! {
    static LISTS: RefCell<HashMap<String, List>> = RefCell::default();
}

/// Starts compiling the list `id` (a stable name: WebKit keeps the compiled form under it)
/// unless it is known already. Main thread only; does nothing elsewhere.
pub fn compile_hosts(id: &str, hosts: &[&str]) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let known = LISTS.with(|lists| lists.borrow().contains_key(id));
    if !known {
        LISTS.with(|lists| lists.borrow_mut().insert(id.to_owned(), List::Compiling(Vec::new())));
        compile(mtm, id, hosts);
    }
}

/// Makes a page view (a live `WKWebView`) block the third-party requests to `hosts` and their
/// subdomains, now or as soon as the list `id` is compiled.
pub fn block_hosts(view: NonNull<c_void>, id: &str, hosts: &[&str]) {
    compile_hosts(id, hosts);
    // Safety: `view` is a live WKWebView used on the main thread; its configuration is a copy
    // that shares the view's user content controller.
    let controller = unsafe {
        let view: &WKWebView = view.cast::<WKWebView>().as_ref();
        view.configuration().userContentController()
    };
    LISTS.with(|lists| match lists.borrow_mut().get_mut(id) {
        // Safety: both objects are alive and used on the main thread.
        Some(List::Ready(list)) => unsafe { controller.addContentRuleList(list) },
        Some(List::Compiling(waiting)) => waiting.push(controller),
        Some(List::Unusable) | None => {}
    });
}

fn compile(mtm: MainThreadMarker, id: &str, hosts: &[&str]) {
    let unusable = |id: &str| {
        LISTS.with(|lists| lists.borrow_mut().insert(id.to_owned(), List::Unusable));
    };
    // Safety: plain accessor on the main thread.
    let (Some(json), Some(store)) = (super::rules_json(hosts), unsafe {
        WKContentRuleListStore::defaultStore(mtm)
    }) else {
        unusable(id);
        return;
    };
    let key = id.to_owned();
    let done = RcBlock::new(move |list: *mut WKContentRuleList, error: *mut NSError| {
        // Safety: WebKit passes a valid list, or nil with an error.
        let list = unsafe { Retained::retain(list) };
        LISTS.with(|lists| {
            let mut lists = lists.borrow_mut();
            let waiting = match lists.remove(&key) {
                Some(List::Compiling(waiting)) => waiting,
                _ => Vec::new(),
            };
            let state = match list {
                Some(list) => {
                    for controller in &waiting {
                        // Safety: live objects, main thread (WebKit calls back there).
                        unsafe { controller.addContentRuleList(&list) };
                    }
                    List::Ready(list)
                }
                None => {
                    // Safety: non-null errors are valid for the duration of the callback.
                    let why = unsafe { error.as_ref() }
                        .map(|e| e.localizedDescription().to_string())
                        .unwrap_or_default();
                    eprintln!("blackhole: content rule list {key} refused: {why}");
                    List::Unusable
                }
            };
            lists.insert(key.clone(), state);
        });
    });
    // Safety: the strings and the block are valid; WebKit copies the block.
    unsafe {
        store.compileContentRuleListForIdentifier_encodedContentRuleList_completionHandler(
            Some(&NSString::from_str(id)),
            Some(&NSString::from_str(&json)),
            Some(&done),
        );
    }
}
