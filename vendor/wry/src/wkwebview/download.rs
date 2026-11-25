use std::{
  cell::RefCell,
  env::current_dir,
  path::PathBuf,
  ptr::null_mut,
  rc::Rc,
};

use objc2::{DeclaredClass, Message, rc::Retained, runtime::ProtocolObject};
use objc2_foundation::{NSData, NSError, NSString, NSURL, NSURLResponse};
use objc2_web_kit::{WKDownload, WKNavigationAction, WKNavigationResponse};

#[cfg(target_os = "ios")]
use crate::wkwebview::ios::WKWebView::WKWebView;
#[cfg(target_os = "macos")]
use objc2_web_kit::WKWebView;

use super::class::{
  wry_download_delegate::WryDownloadDelegate, wry_navigation_delegate::WryNavigationDelegate,
};

fn address(download: &WKDownload) -> usize {
  std::ptr::from_ref(download) as usize
}

/// BlackHole: WebKit has no pause. The download is cancelled with its resume data, and started
/// again from it.
struct MacDownload {
  delegate: Retained<WryDownloadDelegate>,
  download: Rc<RefCell<Retained<WKDownload>>>,
  webview: Option<Retained<WKWebView>>,
  data: Rc<RefCell<Option<Retained<NSData>>>>,
  path: PathBuf,
}

impl crate::site_data::DownloadOps for MacDownload {
  fn pause(&self) -> bool {
    let download = self.download.borrow().clone();
    self.delegate.ivars().stopped.borrow_mut().insert(address(&download));
    let data = self.data.clone();
    let keep = block2::RcBlock::new(move |resume: *mut NSData| {
      *data.borrow_mut() = unsafe { Retained::retain(resume) };
    });
    unsafe { download.cancel(Some(&keep)) };
    true
  }

  fn resume(&self) -> bool {
    let (Some(webview), Some(data)) = (&self.webview, self.data.borrow_mut().take()) else {
      return false;
    };
    let (delegate, path) = (self.delegate.clone(), self.path.clone());
    let current = self.download.clone();
    let start = block2::RcBlock::new(move |download: std::ptr::NonNull<WKDownload>| {
      let download = unsafe { download.as_ref() };
      delegate
        .ivars()
        .resumed
        .borrow_mut()
        .insert(address(download), path.clone());
      unsafe { download.setDelegate(Some(ProtocolObject::from_ref(&*delegate))) };
      *current.borrow_mut() = download.retain();
    });
    unsafe { webview.resumeDownloadFromResumeData_completionHandler(&data, &start) };
    true
  }

  fn cancel(&self) {
    let download = self.download.borrow().clone();
    self.delegate.ivars().stopped.borrow_mut().insert(address(&download));
    unsafe { download.cancel(None) };
  }

  fn can_resume(&self) -> bool {
    self.data.borrow().is_some()
  }

  fn received(&self) -> Option<u64> {
    None
  }
}

// Download action handler
pub(crate) fn navigation_download_action(
  this: &WryNavigationDelegate,
  _webview: &WKWebView,
  _action: &WKNavigationAction,
  download: &WKDownload,
) {
  unsafe {
    if let Some(delegate) = &this.ivars().download_delegate {
      let proto_delegate = ProtocolObject::from_ref(&**delegate);
      download.setDelegate(Some(proto_delegate));
    }
  }
}

// Download response handler
pub(crate) fn navigation_download_response(
  this: &WryNavigationDelegate,
  _webview: &WKWebView,
  _response: &WKNavigationResponse,
  download: &WKDownload,
) {
  unsafe {
    if let Some(delegate) = &this.ivars().download_delegate {
      let proto_delegate = ProtocolObject::from_ref(&**delegate);
      download.setDelegate(Some(proto_delegate));
    }
  }
}

pub(crate) fn download_policy(
  this: &WryDownloadDelegate,
  download: &WKDownload,
  _response: &NSURLResponse,
  suggested_filename: &NSString,
  completion_handler: &block2::Block<dyn Fn(*const NSURL)>,
) {
  unsafe {
    // BlackHole: a resumed download goes on in its own file.
    let known = this.ivars().resumed.borrow().get(&address(download)).cloned();
    if let Some(path) = known {
      let path = NSString::from_str(&path.display().to_string());
      let ns_url = NSURL::fileURLWithPath_isDirectory(&path, false);
      (*completion_handler).call((Retained::as_ptr(&ns_url),));
      return;
    }
    let request = download.originalRequest().unwrap();
    let url = request.URL().unwrap().absoluteString().unwrap();
    let suggested_filename = suggested_filename.to_string();
    let mut download_destination =
      dirs::download_dir().unwrap_or_else(|| current_dir().unwrap_or_default());

    download_destination.push(&suggested_filename);

    let (suggested_filename, ext) = suggested_filename
      .split_once('.')
      .map(|(base, ext)| (base, format!(".{ext}")))
      .unwrap_or((&suggested_filename, "".to_string()));

    // WebView2 does not overwrite files but appends numbers
    let mut counter = 1;
    while download_destination.exists() {
      download_destination.set_file_name(format!("{suggested_filename} ({counter}){ext}"));
      counter += 1;
    }

    let started_fn = &this.ivars().started;
    if let Some(started_fn) = started_fn {
      let mut started_fn = started_fn.borrow_mut();
      match started_fn(url.to_string(), &mut download_destination) {
        true => {
          if let Some(control) = &this.ivars().control {
            let ops = MacDownload {
              delegate: this.retain(),
              download: Rc::new(RefCell::new(download.retain())),
              webview: download.webView(),
              data: Rc::default(),
              path: download_destination.clone(),
            };
            control(
              download_destination.clone(),
              crate::DownloadControl { ops: Rc::new(ops) },
            );
          }
          let path = NSString::from_str(&download_destination.display().to_string());
          let ns_url = NSURL::fileURLWithPath_isDirectory(&path, false);
          (*completion_handler).call((Retained::as_ptr(&ns_url),))
        }
        false => (*completion_handler).call((null_mut(),)),
      };
    } else {
      #[cfg(feature = "tracing")]
      tracing::warn!("WebView instance is dropped! This navigation handler shouldn't be called.");
      (*completion_handler).call((null_mut(),));
    }
  }
}

pub(crate) fn download_did_finish(this: &WryDownloadDelegate, download: &WKDownload) {
  unsafe {
    let original_request = download.originalRequest().unwrap();
    let url = original_request.URL().unwrap().absoluteString().unwrap();
    if let Some(completed_fn) = this.ivars().completed.clone() {
      completed_fn(url.to_string(), None, true);
    }
  }
}

pub(crate) fn download_did_fail(
  this: &WryDownloadDelegate,
  download: &WKDownload,
  error: &NSError,
  _resume_data: &NSData,
) {
  unsafe {
    #[cfg(debug_assertions)]
    {
      let description = error.localizedDescription().to_string();
      eprintln!("Download failed with error: {description}");
    }

    if this.ivars().stopped.borrow_mut().remove(&address(download)) {
      return;
    }
    let original_request = download.originalRequest().unwrap();
    let url = original_request.URL().unwrap().absoluteString().unwrap();
    if let Some(completed_fn) = this.ivars().completed.clone() {
      completed_fn(url.to_string(), None, false);
    }
  }
}
