// Copyright 2020-2024 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use std::sync::{Arc, Mutex};

use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send, rc::Retained, runtime::NSObject};
use objc2_foundation::{
  MainThreadMarker, NSError, NSObjectProtocol, NSURLAuthenticationChallenge, NSURLCredential,
  NSURLSessionAuthChallengeDisposition,
};
use objc2_web_kit::{
  WKDownload, WKNavigation, WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate,
  WKNavigationResponse, WKNavigationResponsePolicy,
};

#[cfg(target_os = "ios")]
use crate::wkwebview::ios::WKWebView::WKWebView;
#[cfg(target_os = "macos")]
use objc2_web_kit::WKWebView;

use crate::{
  HttpAuthChallenge, HttpAuthResponder, LoadFailure, PageLoadEvent, ServerTrustChallenge,
  WryWebView, url_from_webview,
  wkwebview::{
    download::{navigation_download_action, navigation_download_response},
    navigation::{
      did_commit_navigation, did_fail_navigation, did_finish_navigation, did_receive_authentication_challenge,
      navigation_policy, navigation_policy_response, web_content_process_did_terminate,
    },
  },
};

use super::wry_download_delegate::WryDownloadDelegate;

pub struct WryNavigationDelegateIvars {
  pub pending_scripts: Arc<Mutex<Option<Vec<String>>>>,
  pub has_download_handler: bool,
  pub navigation_policy_function: Box<dyn Fn(String) -> bool>,
  pub download_delegate: Option<Retained<WryDownloadDelegate>>,
  pub on_page_load_handler: Option<Box<dyn Fn(PageLoadEvent, Option<String>)>>,
  pub on_web_content_process_terminate_handler: Option<Box<dyn Fn()>>,
  pub content_blocked_handler: Option<Box<dyn Fn(String)>>,
  pub http_auth_handler: Option<Box<dyn Fn(HttpAuthChallenge, HttpAuthResponder)>>,
  pub load_failure_handler: Option<Box<dyn Fn(LoadFailure)>>,
  pub server_trust_handler: Option<Box<dyn Fn(ServerTrustChallenge) -> bool>>,
}

define_class!(
  #[unsafe(super(NSObject))]
  #[thread_kind = MainThreadOnly]
  #[ivars = WryNavigationDelegateIvars]
  pub struct WryNavigationDelegate;

  unsafe impl NSObjectProtocol for WryNavigationDelegate {}

  unsafe impl WKNavigationDelegate for WryNavigationDelegate {
    #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
    fn navigation_policy(
      &self,
      webview: &WKWebView,
      action: &WKNavigationAction,
      handler: &block2::Block<dyn Fn(WKNavigationActionPolicy)>,
    ) {
      navigation_policy(self, webview, action, handler);
    }

    #[unsafe(method(webView:decidePolicyForNavigationResponse:decisionHandler:))]
    fn navigation_policy_response(
      &self,
      webview: &WKWebView,
      response: &WKNavigationResponse,
      handler: &block2::Block<dyn Fn(WKNavigationResponsePolicy)>,
    ) {
      navigation_policy_response(self, webview, response, handler);
    }

    #[unsafe(method(webView:didFinishNavigation:))]
    fn did_finish_navigation(&self, webview: &WKWebView, navigation: &WKNavigation) {
      did_finish_navigation(self, webview, navigation);
    }

    #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
    fn did_fail_provisional_navigation(
      &self,
      webview: &WKWebView,
      navigation: &WKNavigation,
      error: &NSError,
    ) {
      did_fail_navigation(self, webview, navigation, error);
    }

    #[unsafe(method(webView:didFailNavigation:withError:))]
    fn did_fail_navigation(
      &self,
      webview: &WKWebView,
      navigation: &WKNavigation,
      error: &NSError,
    ) {
      did_fail_navigation(self, webview, navigation, error);
    }

    #[unsafe(method(webView:didCommitNavigation:))]
    fn did_commit_navigation(&self, webview: &WKWebView, navigation: &WKNavigation) {
      did_commit_navigation(self, webview, navigation);
    }

    #[unsafe(method(webView:navigationAction:didBecomeDownload:))]
    fn navigation_download_action(
      &self,
      webview: &WKWebView,
      action: &WKNavigationAction,
      download: &WKDownload,
    ) {
      navigation_download_action(self, webview, action, download);
    }

    #[unsafe(method(webView:navigationResponse:didBecomeDownload:))]
    fn navigation_download_response(
      &self,
      webview: &WKWebView,
      response: &WKNavigationResponse,
      download: &WKDownload,
    ) {
      navigation_download_response(self, webview, response, download);
    }

    #[unsafe(method(webView:didReceiveAuthenticationChallenge:completionHandler:))]
    fn did_receive_authentication_challenge(
      &self,
      webview: &WKWebView,
      challenge: &NSURLAuthenticationChallenge,
      handler: &block2::Block<dyn Fn(NSURLSessionAuthChallengeDisposition, *mut NSURLCredential)>,
    ) {
      did_receive_authentication_challenge(self, webview, challenge, handler);
    }

    #[unsafe(method(webViewWebContentProcessDidTerminate:))]
    fn web_content_process_did_terminate(&self, webview: &WKWebView) {
      web_content_process_did_terminate(self, webview);
    }
  }

  /// BlackHole: `WKNavigationDelegatePrivate`. WebKit calls it, when the delegate has it, for
  /// each request a content rule list acted on (`action` is a `_WKContentRuleListAction`).
  impl WryNavigationDelegate {
    #[unsafe(method(_webView:contentRuleListWithIdentifier:performedAction:forURL:))]
    fn content_rule_list_action(
      &self,
      _webview: &WKWebView,
      _identifier: &objc2_foundation::NSString,
      action: &objc2::runtime::AnyObject,
      url: &objc2_foundation::NSURL,
    ) {
      let Some(report) = &self.ivars().content_blocked_handler else {
        return;
      };
      let blocked: bool = unsafe {
        let known: bool = msg_send![action, respondsToSelector: objc2::sel!(blockedLoad)];
        known && msg_send![action, blockedLoad]
      };
      if let (true, Some(url)) = (blocked, url.absoluteString()) {
        report(url.to_string());
      }
    }
  }
);

impl WryNavigationDelegate {
  #[allow(clippy::too_many_arguments)]
  pub fn new(
    webview: Retained<WryWebView>,
    pending_scripts: Arc<Mutex<Option<Vec<String>>>>,
    has_download_handler: bool,
    navigation_handler: Option<Box<dyn Fn(String) -> bool>>,
    download_delegate: Option<Retained<WryDownloadDelegate>>,
    on_page_load_handler: Option<Box<dyn Fn(PageLoadEvent, String)>>,
    on_web_content_process_terminate_handler: Option<Box<dyn Fn()>>,
    content_blocked_handler: Option<Box<dyn Fn(String)>>,
    http_auth_handler: Option<Box<dyn Fn(HttpAuthChallenge, HttpAuthResponder)>>,
    load_failure_handler: Option<Box<dyn Fn(LoadFailure)>>,
    server_trust_handler: Option<Box<dyn Fn(ServerTrustChallenge) -> bool>>,
    mtm: MainThreadMarker,
  ) -> Retained<Self> {
    let navigation_policy_function = Box::new(move |url: String| -> bool {
      navigation_handler
        .as_ref()
        .is_none_or(|navigation_handler| (navigation_handler)(url))
    });

    let on_page_load_handler = if let Some(handler) = on_page_load_handler {
      let custom_handler = Box::new(move |event, url: Option<String>| {
        let url = url.unwrap_or_else(|| url_from_webview(&webview).unwrap_or_default());
        handler(event, url);
      }) as Box<dyn Fn(PageLoadEvent, Option<String>)>;
      Some(custom_handler)
    } else {
      None
    };

    let on_web_content_process_terminate_handler =
      if let Some(handler) = on_web_content_process_terminate_handler {
        let custom_handler = Box::new(move || {
          handler();
        }) as Box<dyn Fn()>;
        Some(custom_handler)
      } else {
        None
      };

    let delegate = mtm
      .alloc::<WryNavigationDelegate>()
      .set_ivars(WryNavigationDelegateIvars {
        pending_scripts,
        navigation_policy_function,
        has_download_handler,
        download_delegate,
        on_page_load_handler,
        on_web_content_process_terminate_handler,
        content_blocked_handler,
        http_auth_handler,
        load_failure_handler,
        server_trust_handler,
      });

    unsafe { msg_send![super(delegate), init] }
  }
}
