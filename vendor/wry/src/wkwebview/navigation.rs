use objc2::DeclaredClass;
use objc2_foundation::{NSError, NSObjectProtocol, NSString};
use objc2_web_kit::{
  WKNavigation, WKNavigationAction, WKNavigationActionPolicy, WKNavigationResponse,
  WKNavigationResponsePolicy,
};

#[cfg(target_os = "ios")]
use crate::wkwebview::ios::WKWebView::WKWebView;
#[cfg(target_os = "macos")]
use objc2_web_kit::WKWebView;

use crate::PageLoadEvent;

#[cfg(target_os = "macos")]
pub static USER_AGENT_RESOLVER: std::sync::OnceLock<fn(&str) -> Option<String>> =
  std::sync::OnceLock::new();

use super::class::wry_navigation_delegate::WryNavigationDelegate;

pub(crate) fn did_commit_navigation(
  this: &WryNavigationDelegate,
  webview: &WKWebView,
  _navigation: &WKNavigation,
) {
  unsafe {
    // Call on_load_handler
    if let Some(on_page_load) = &this.ivars().on_page_load_handler {
      on_page_load(PageLoadEvent::Started, None);
    }

    // Inject scripts
    let mut pending_scripts = this.ivars().pending_scripts.lock().unwrap();
    if let Some(scripts) = &*pending_scripts {
      for script in scripts {
        #[cfg(feature = "tracing")]
        let handler = block2::RcBlock::new(
          |_val: *mut objc2::runtime::AnyObject, err: *mut objc2_foundation::NSError| {
            if !err.is_null() {
              let description = (*err).localizedDescription().to_string();
              tracing::debug!("Exception during script eval: {description}");
            }
          },
        );
        #[cfg(feature = "tracing")]
        let handler = Some(&*handler);
        #[cfg(not(feature = "tracing"))]
        let handler = None;

        webview.evaluateJavaScript_completionHandler(&NSString::from_str(script), handler);
      }
      *pending_scripts = None;
    }
  }
}

pub(crate) fn did_finish_navigation(
  this: &WryNavigationDelegate,
  _webview: &WKWebView,
  _navigation: &WKNavigation,
) {
  if let Some(on_page_load) = &this.ivars().on_page_load_handler {
    on_page_load(PageLoadEvent::Finished, None);
  }
}

// Navigation handler
pub(crate) fn navigation_policy(
  this: &WryNavigationDelegate,
  webview: &WKWebView,
  action: &WKNavigationAction,
  handler: &block2::Block<dyn Fn(WKNavigationActionPolicy)>,
) {
  unsafe {
    #[cfg(target_os = "macos")]
    if let Some(resolve) = USER_AGENT_RESOLVER.get() {
      let main_frame = action.targetFrame().is_none_or(|f| f.isMainFrame());
      if main_frame {
        if let Some(url) = action.request().URL().and_then(|u| u.absoluteString()) {
          let ua = resolve(&url.to_string());
          webview.setCustomUserAgent(ua.as_deref().map(NSString::from_str).as_deref());
        }
      }
    }
    // <https://developer.apple.com/documentation/webkit/wknavigationaction/shouldperformdownload>
    // Available: macOS 11.3+, iOS 14.5+
    let can_download = action.respondsToSelector(objc2::sel!(shouldPerformDownload));
    let should_download: bool = if can_download {
      action.shouldPerformDownload()
    } else {
      false
    };
    let request = action.request();
    let url = request.URL().unwrap().absoluteString().unwrap();

    if should_download {
      let has_download_handler = this.ivars().has_download_handler;
      if has_download_handler {
        (*handler).call((WKNavigationActionPolicy::Download,));
      } else {
        (*handler).call((WKNavigationActionPolicy::Cancel,));
      }
    } else {
      let function = &this.ivars().navigation_policy_function;
      match function(url.to_string()) {
        true => (*handler).call((WKNavigationActionPolicy::Allow,)),
        false => (*handler).call((WKNavigationActionPolicy::Cancel,)),
      };
    }
  }
}

// Navigation handler
pub(crate) fn navigation_policy_response(
  this: &WryNavigationDelegate,
  _webview: &WKWebView,
  response: &WKNavigationResponse,
  handler: &block2::Block<dyn Fn(WKNavigationResponsePolicy)>,
) {
  unsafe {
    let can_show_mime_type = response.canShowMIMEType();

    if !can_show_mime_type {
      let has_download_handler = this.ivars().has_download_handler;
      if has_download_handler {
        (*handler).call((WKNavigationResponsePolicy::Download,));
        return;
      }
    }

    (*handler).call((WKNavigationResponsePolicy::Allow,));
  }
}

pub(crate) fn web_content_process_did_terminate(
  this: &WryNavigationDelegate,
  _webview: &WKWebView,
) {
  if let Some(on_web_content_process_terminate) =
    &this.ivars().on_web_content_process_terminate_handler
  {
    on_web_content_process_terminate();
  }
}

pub(crate) fn did_fail_navigation(
  this: &WryNavigationDelegate,
  _webview: &WKWebView,
  _navigation: &WKNavigation,
  error: &NSError,
) {
  // NSURLErrorCancelled: superseded navigation or one converted into a download.
  if error.code() == -999 {
    return;
  }
  let url = error
    .userInfo()
    .objectForKey(&NSString::from_str("NSErrorFailingURLStringKey"))
    .and_then(|v| v.downcast::<NSString>().ok())
    .map(|s| s.to_string());
  if let Some(on_page_load) = &this.ivars().on_page_load_handler {
    on_page_load(PageLoadEvent::Failed, url);
  }
}
