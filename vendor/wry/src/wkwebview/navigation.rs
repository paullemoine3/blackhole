use objc2::DeclaredClass;
use objc2_foundation::{
  NSError, NSHTTPURLResponse, NSObjectProtocol, NSString, NSURLAuthenticationChallenge,
  NSURLAuthenticationMethodHTTPBasic, NSURLAuthenticationMethodHTTPDigest,
  NSURLAuthenticationMethodServerTrust, NSURLCredential, NSURLCredentialPersistence,
  NSURLSessionAuthChallengeDisposition,
};
use objc2_web_kit::{
  WKNavigation, WKNavigationAction, WKNavigationActionPolicy, WKNavigationResponse,
  WKNavigationResponsePolicy,
};

#[cfg(target_os = "ios")]
use crate::wkwebview::ios::WKWebView::WKWebView;
#[cfg(target_os = "macos")]
use objc2_web_kit::WKWebView;

use crate::{HttpAuthChallenge, HttpAuthResponder, LoadFailure, PageLoadEvent, ServerTrustChallenge};

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
      // No target frame = a new window: it opens in another view, which picks its own agent;
      // the opener keeps its one.
      let main_frame = action.targetFrame().is_some_and(|f| f.isMainFrame());
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
      // No target frame (a new window) counts as not the main frame of this view.
      crate::set_navigating_main_frame(action.targetFrame().is_some_and(|f| f.isMainFrame()));
      let allowed = function(url.to_string());
      crate::set_navigating_main_frame(true);
      match allowed {
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
    if let (true, Some(report)) = (response.isForMainFrame(), &this.ivars().load_failure_handler) {
      let answer = response.response();
      if let Ok(http) = answer.downcast::<NSHTTPURLResponse>() {
        let status = http.statusCode();
        if status >= 400 {
          report(LoadFailure {
            url: http
              .URL()
              .and_then(|u| u.absoluteString())
              .map(|s| s.to_string())
              .unwrap_or_default(),
            domain: "http".to_owned(),
            code: status as i64,
            http_status: u16::try_from(status).unwrap_or(0),
          });
        }
      }
    }
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
  let url = error
    .userInfo()
    .objectForKey(&NSString::from_str("NSErrorFailingURLStringKey"))
    .and_then(|v| v.downcast::<NSString>().ok())
    .map(|s| s.to_string());
  if let Some(report) = &this.ivars().load_failure_handler {
    report(LoadFailure {
      url: url.clone().unwrap_or_default(),
      domain: error.domain().to_string(),
      code: error.code() as i64,
      http_status: 0,
    });
  }
  // NSURLErrorCancelled: superseded navigation or one converted into a download.
  if error.code() == -999 {
    return;
  }
  if let Some(on_page_load) = &this.ivars().on_page_load_handler {
    on_page_load(PageLoadEvent::Failed, url);
  }
}

/// Basic and Digest challenges go to the app's handler; anything else (server trust, client
/// certificates, NTLM…) keeps WebKit's default handling.
pub(crate) fn did_receive_authentication_challenge(
  this: &WryNavigationDelegate,
  _webview: &WKWebView,
  challenge: &NSURLAuthenticationChallenge,
  handler: &block2::Block<dyn Fn(NSURLSessionAuthChallengeDisposition, *mut NSURLCredential)>,
) {
  unsafe {
    let space = challenge.protectionSpace();
    let method = space.authenticationMethod();
    if method.isEqualToString(NSURLAuthenticationMethodServerTrust) {
      server_trust(this, challenge, handler);
      return;
    }
    let http = method.isEqualToString(NSURLAuthenticationMethodHTTPBasic)
      || method.isEqualToString(NSURLAuthenticationMethodHTTPDigest);
    let Some(auth_handler) = this.ivars().http_auth_handler.as_ref().filter(|_| http) else {
      handler.call((
        NSURLSessionAuthChallengeDisposition::PerformDefaultHandling,
        std::ptr::null_mut(),
      ));
      return;
    };
    let host = space.host().to_string();
    let scheme = space.protocol().map(|p| p.to_string()).unwrap_or_else(|| "https".to_owned());
    let port = space.port();
    let url = match (scheme.as_str(), port) {
      ("https", 443) | ("http", 80) | (_, 0) => format!("{scheme}://{host}/"),
      _ => format!("{scheme}://{host}:{port}/"),
    };
    let challenge_info = HttpAuthChallenge {
      url,
      host,
      challenge: space.realm().map(|r| r.to_string()).unwrap_or_default(),
      secure: space.receivesCredentialSecurely(),
      retry: challenge.previousFailureCount() > 0,
    };
    // The answer may come later (the user types it): keep the completion handler alive.
    let handler = handler.copy();
    let responder = HttpAuthResponder::new(move |credentials| match credentials {
      Some((user, password)) => {
        // Kept in memory for this session only, never written to the keychain.
        let credential = NSURLCredential::credentialWithUser_password_persistence(
          &NSString::from_str(&user),
          &NSString::from_str(&password),
          NSURLCredentialPersistence::ForSession,
        );
        handler.call((
          NSURLSessionAuthChallengeDisposition::UseCredential,
          objc2::rc::Retained::as_ptr(&credential).cast_mut(),
        ));
      }
      // Going on without credentials shows the server's own 401 page.
      None => handler.call((
        NSURLSessionAuthChallengeDisposition::UseCredential,
        std::ptr::null_mut(),
      )),
    });
    auth_handler(challenge_info, responder);
  }
}

/// `SecTrustRef`, typed so that the message encoding matches (`^{__SecTrust=}`).
#[repr(transparent)]
#[derive(Clone, Copy)]
struct SecTrustRef(*mut std::ffi::c_void);

// SAFETY: a pointer to the opaque `__SecTrust` struct, as WebKit declares it.
unsafe impl objc2::Encode for SecTrustRef {
  const ENCODING: objc2::Encoding =
    objc2::Encoding::Pointer(&objc2::Encoding::Struct("__SecTrust", &[]));
}

/// A server certificate: WebKit evaluates it by default; only a host the app's handler trusts
/// (the user chose to go on) is answered with a credential for that trust.
fn server_trust(
  this: &WryNavigationDelegate,
  challenge: &NSURLAuthenticationChallenge,
  handler: &block2::Block<dyn Fn(NSURLSessionAuthChallengeDisposition, *mut NSURLCredential)>,
) {
  unsafe {
    let space = challenge.protectionSpace();
    let host = space.host().to_string();
    let port = space.port();
    let url = if port == 443 || port == 0 {
      format!("https://{host}/")
    } else {
      format!("https://{host}:{port}/")
    };
    let trusted = this.ivars().server_trust_handler.as_ref().is_some_and(|h| {
      h(ServerTrustChallenge {
        url,
        host,
        ..Default::default()
      })
    });
    let trust: SecTrustRef = objc2::msg_send![&*space, serverTrust];
    if trusted && !trust.0.is_null() {
      let credential: objc2::rc::Retained<NSURLCredential> =
        objc2::msg_send![objc2::class!(NSURLCredential), credentialForTrust: trust];
      handler.call((
        NSURLSessionAuthChallengeDisposition::UseCredential,
        objc2::rc::Retained::as_ptr(&credential).cast_mut(),
      ));
    } else {
      handler.call((
        NSURLSessionAuthChallengeDisposition::PerformDefaultHandling,
        std::ptr::null_mut(),
      ));
    }
  }
}
